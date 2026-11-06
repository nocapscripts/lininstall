//! Tauri commands: the Rust side of what used to be `main.js` + `preload.js`.

use crate::detect::{self, Ctx};
use crate::tasks::{Task, CATALOG};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

#[derive(Default)]
pub struct AppState {
    ctx: Mutex<Option<Ctx>>,
    running: AtomicBool,
}

// ---------- data sent to the UI ----------

#[derive(Serialize)]
pub struct TaskView {
    id: &'static str,
    name: String,
    desc: String,
    group: &'static str,
    root: bool,
    reboot: bool,
    danger: bool,
    cmd: Option<String>,
}

#[derive(Serialize)]
pub struct CategoryView {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    tasks: Vec<TaskView>,
}

#[derive(Serialize)]
pub struct Payload {
    info: Ctx,
    categories: Vec<CategoryView>,
}

#[derive(Serialize)]
pub struct RunResult {
    code: i32,
    busy: bool,
    reboot: bool,
}

impl RunResult {
    fn code(code: i32) -> Self { Self { code, busy: false, reboot: false } }
    fn busy() -> Self { Self { code: -1, busy: true, reboot: false } }
}

#[derive(Serialize)]
pub struct FlatpakApp {
    id: String,
    name: String,
}

#[derive(Serialize)]
pub struct Packages {
    native: Vec<String>,
    flatpak: Vec<FlatpakApp>,
}

#[derive(Deserialize, Default)]
pub struct Selection {
    #[serde(default)]
    native: Vec<String>,
    #[serde(default)]
    flatpak: Vec<String>,
}

// ---------- helpers ----------

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid regex")
}

/// Package managers that work in user space (no root)
const USER_ONLY_PMS: [&str; 2] = ["brew", "nix"];
static USER_CMD_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"^\s*(brew|nix|yay|paru)\b|^\s*H=\$\(command -v yay"));
static PKG_RE: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9][A-Za-z0-9+._:@/-]*$"));
/// Never offered for removal (use the GPU Drivers page for NVIDIA)
static PROTECTED: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(kernel.*|linux|linux-.*|systemd.*|glibc.*|bash|sudo|polkit.*|dnf.*|rpm.*|apt.*|dpkg|pacman|zypper|coreutils.*|NetworkManager.*|grub.*|shim.*|dracut.*|mutter|gnome-shell|plasma-.*|gdm|sddm|lightdm|cinnamon.*|xorg-x11-server.*|selinux.*|firewalld|nvidia.*|akmod.*|xorg-x11-drv.*|apk-tools.*|xbps.*|portage.*|busybox|musl.*|openrc.*|nix|brew|lininstall)$")
});

fn resolve_cmd(task: &Task, ctx: &Ctx) -> Option<String> {
    if task.sys && USER_ONLY_PMS.contains(&ctx.pm.as_str()) {
        return None;
    }
    (task.cmd)(ctx).filter(|s| !s.is_empty())
}

fn is_root(task: &Task, cmd: &str) -> bool {
    task.root && !USER_CMD_RE.is_match(cmd)
}

fn payload(ctx: Ctx) -> Payload {
    let categories = CATALOG
        .iter()
        .map(|c| CategoryView {
            id: c.id,
            name: c.name,
            icon: c.icon,
            tasks: c
                .tasks
                .iter()
                .map(|t| {
                    let cmd = resolve_cmd(t, &ctx);
                    TaskView {
                        id: t.id,
                        name: t.name.clone(),
                        desc: t.desc.clone(),
                        group: t.group,
                        root: is_root(t, cmd.as_deref().unwrap_or("")),
                        reboot: t.reboot,
                        danger: t.danger,
                        cmd,
                    }
                })
                .collect(),
        })
        .collect();
    Payload { info: ctx, categories }
}

fn log(app: &AppHandle, text: &str) {
    let _ = app.emit("log", text);
}

// ---------- settings ----------

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("settings.json"))
}

fn load_pm_choice(app: &AppHandle) -> String {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["pmChoice"].as_str().map(String::from))
        .unwrap_or_else(|| "auto".to_string())
}

fn save_pm_choice(app: &AppHandle, choice: &str) {
    if let Some(path) = settings_path(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::json!({ "pmChoice": choice }).to_string());
    }
}

/// Detect the system once (off the UI thread) and cache it.
async fn ensure_ctx(app: &AppHandle, st: &AppState) -> Result<Ctx, String> {
    let cached = st.ctx.lock().unwrap().clone();
    if let Some(c) = cached {
        return Ok(c);
    }
    let mut ctx = tauri::async_runtime::spawn_blocking(detect::detect)
        .await
        .map_err(|e| e.to_string())?;
    detect::apply_choice(&mut ctx, &load_pm_choice(app));
    *st.ctx.lock().unwrap() = Some(ctx.clone());
    Ok(ctx)
}

// ---------- running commands ----------

/// Only one command at a time; the lock is released when the guard is dropped.
struct RunGuard<'a>(&'a AtomicBool);

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn try_lock(flag: &AtomicBool) -> Option<RunGuard<'_>> {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| RunGuard(flag))
}

/// Forward a child's output to the UI as it arrives.
fn pump<R>(app: AppHandle, reader: Option<R>) -> tauri::async_runtime::JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let Some(mut r) = reader else { return };
        let mut buf = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    // Don't cut a multi-byte character in half between two reads
                    let cut = match std::str::from_utf8(&pending) {
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        _ => pending.len(),
                    };
                    let chunk: Vec<u8> = pending.drain(..cut).collect();
                    if !chunk.is_empty() {
                        log(&app, &String::from_utf8_lossy(&chunk));
                    }
                }
            }
        }
        if !pending.is_empty() {
            log(&app, &String::from_utf8_lossy(&pending));
        }
    })
}

/// Run a bash script (through pkexec when `root`), streaming output. `None` = failed to start.
async fn run_script(app: &AppHandle, script: &str, root: bool) -> Option<i32> {
    let mut cmd = if root {
        let mut c = Command::new("pkexec"); // graphical password prompt
        c.args(["bash", "-c", script]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-c", script]);
        c
    };
    cmd.env("PATH", detect::extended_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log(app, &format!("Error: {e}\n"));
            return None;
        }
    };
    let out = pump(app.clone(), child.stdout.take());
    let err = pump(app.clone(), child.stderr.take());
    let status = child.wait().await;
    let _ = out.await;
    let _ = err.await;
    match status {
        Ok(s) => Some(s.code().unwrap_or(-1)),
        Err(e) => {
            log(app, &format!("Error: {e}\n"));
            None
        }
    }
}

/// Lock, print a header, run, print the result line.
async fn exec(app: &AppHandle, running: &AtomicBool, header: String, script: &str, root: bool) -> RunResult {
    let Some(_guard) = try_lock(running) else {
        return RunResult::busy();
    };
    log(app, &header);
    match run_script(app, script, root).await {
        None => RunResult::code(1),
        Some(code) => {
            let line = if code == 0 {
                "\n[✔ finished]\n".to_string()
            } else {
                format!("\n[✖ exited with code {code}]\n")
            };
            log(app, &line);
            RunResult::code(code)
        }
    }
}

/// Run a shell command and capture stdout (used for listing packages).
async fn sh(cmd: &str) -> String {
    Command::new("sh")
        .args(["-c", cmd])
        .env("PATH", detect::extended_path())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

// ---------- commands ----------

#[tauri::command]
pub async fn get_data(app: AppHandle, state: State<'_, AppState>) -> Result<Payload, String> {
    Ok(payload(ensure_ctx(&app, &state).await?))
}

#[tauri::command]
pub async fn refresh(app: AppHandle, state: State<'_, AppState>) -> Result<Payload, String> {
    *state.ctx.lock().unwrap() = None;
    Ok(payload(ensure_ctx(&app, &state).await?))
}

#[tauri::command]
pub async fn set_pm(app: AppHandle, state: State<'_, AppState>, choice: String) -> Result<Payload, String> {
    let mut ctx = ensure_ctx(&app, &state).await?;
    detect::apply_choice(&mut ctx, &choice);
    save_pm_choice(&app, &ctx.pm_choice);
    *state.ctx.lock().unwrap() = Some(ctx.clone());
    Ok(payload(ctx))
}

#[tauri::command]
pub async fn run_task(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<RunResult, String> {
    if state.running.load(Ordering::SeqCst) {
        return Ok(RunResult::busy());
    }
    let ctx = ensure_ctx(&app, &state).await?;
    // Only tasks from the built-in catalog can run: the UI sends an id, never a command.
    let task = CATALOG.iter().flat_map(|c| c.tasks.iter()).find(|t| t.id == id);
    let (Some(task), Some(cmd)) = (task, task.and_then(|t| resolve_cmd(t, &ctx))) else {
        log(&app, "\nThis task is not supported on your distro.\n");
        return Ok(RunResult::code(1));
    };

    let header = format!("\n━━ {} ━━\n", task.name);
    let mut result = exec(&app, &state.running, header, &cmd, is_root(task, &cmd)).await;
    if result.code == 0 && task.reboot {
        log(&app, "[↻ A reboot is recommended]\n");
        result.reboot = true;
    }
    Ok(result)
}

#[tauri::command]
pub async fn list_packages(state: State<'_, AppState>) -> Result<Packages, String> {
    let pm = state.ctx.lock().unwrap().as_ref().map(|c| c.pm.clone()).unwrap_or_default();
    let list_cmd = match pm.as_str() {
        "dnf" => "dnf repoquery --userinstalled --queryformat '%{name}\\n' 2>/dev/null | sort -u",
        "apt" => "apt-mark showmanual | sort -u",
        "pacman" | "yay" | "paru" => "pacman -Qqe | sort -u",
        "zypper" => "rpm -qa --qf '%{NAME}\\n' | sort -u",
        "apk" => "sed 's/[<>=~].*//' /etc/apk/world | sort -u",
        "xbps" => "xbps-query -m | sed 's/-[^-]*$//' | sort -u",
        "emerge" => "sed 's/[<>=~].*//' /var/lib/portage/world | sort -u",
        "brew" => "brew leaves",
        _ => "true",
    };
    let to_lines = |s: String| -> Vec<String> {
        s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    };

    let mut native = to_lines(sh(list_cmd).await);
    if native.is_empty() && (pm == "dnf" || pm == "zypper") {
        native = to_lines(sh("rpm -qa --qf '%{NAME}\\n' | sort -u").await);
    }
    native.retain(|n| PKG_RE.is_match(n) && !PROTECTED.is_match(n));

    let flatpak = sh("flatpak list --app --columns=application,name 2>/dev/null")
        .await
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut parts = l.split('\t');
            let id = parts.next().unwrap_or("").trim().to_string();
            let name = parts.next().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
            FlatpakApp { name: name.unwrap_or_else(|| id.clone()), id }
        })
        .filter(|f| PKG_RE.is_match(&f.id))
        .collect();

    Ok(Packages { native, flatpak })
}

#[tauri::command]
pub async fn remove_packages(
    app: AppHandle,
    state: State<'_, AppState>,
    sel: Selection,
    dry: bool,
) -> Result<RunResult, String> {
    let pm = ensure_ctx(&app, &state).await?.pm;
    // Re-validate everything: the UI is not trusted to have filtered the list.
    let native: Vec<String> = sel
        .native
        .into_iter()
        .filter(|n| PKG_RE.is_match(n) && !PROTECTED.is_match(n))
        .collect();
    let flatpak: Vec<String> = sel.flatpak.into_iter().filter(|f| PKG_RE.is_match(f)).collect();
    let l = native.join(" ");

    let real: Option<String> = match pm.as_str() {
        "dnf" => Some(format!("dnf remove -y {l}")),
        "apt" => Some(format!("export DEBIAN_FRONTEND=noninteractive; apt-get purge -y {l} && apt-get autoremove -y")),
        "pacman" => Some(format!("pacman -Rns --noconfirm {l}")),
        "zypper" => Some(format!("zypper --non-interactive remove {l}")),
        "apk" => Some(format!("apk del {l}")),
        "xbps" => Some(format!("xbps-remove -Ry {l}")),
        "emerge" => Some(format!("emerge --ask=n --depclean {l}")),
        "brew" => Some(format!("brew uninstall {l}")),
        "yay" => Some(format!("yay -Rns --noconfirm --sudo pkexec {l}")),
        "paru" => Some(format!("paru -Rns --noconfirm --sudo pkexec {l}")),
        _ => None,
    };
    let preview: Option<String> = match pm.as_str() {
        "dnf" => Some(format!("dnf remove --assumeno {l}")),
        "apt" => Some(format!("apt-get -s purge {l}")),
        "pacman" | "yay" | "paru" => Some(format!("pacman -Rnsp {l}")),
        "zypper" => Some(format!("zypper --non-interactive remove --dry-run {l}")),
        "apk" => Some(format!("apk del -s {l}")),
        "xbps" => Some(format!("xbps-remove -Rn {l}")),
        "emerge" => Some(format!("emerge -pc {l}")),
        "brew" => Some(format!(r#"echo "Would uninstall: {l}""#)),
        _ => None,
    };

    if !native.is_empty() && real.is_none() {
        log(&app, &format!("\nRemoving packages is not supported for {pm}.\n"));
        return Ok(RunResult::code(1));
    }

    if dry {
        let Some(preview) = preview.filter(|_| !native.is_empty()) else {
            return Ok(RunResult::code(0));
        };
        let header = format!("\n━━ Preview: what would be removed ━━\n$ {preview}\n");
        return Ok(exec(&app, &state.running, header, &format!("{preview}; true"), false).await);
    }

    let mut result = RunResult::code(0);
    if let Some(real) = real.filter(|_| !native.is_empty()) {
        let header = format!("\n━━ Remove packages ━━\n$ {real}\n");
        let root = !USER_CMD_RE.is_match(&real);
        result = exec(&app, &state.running, header, &real, root).await;
    }
    if !flatpak.is_empty() && result.code == 0 {
        let script = format!("flatpak uninstall -y {}", flatpak.join(" "));
        let header = format!("\n━━ Remove Flatpak apps ━━\n$ {script}\n");
        result = exec(&app, &state.running, header, &script, false).await;
    }
    Ok(result)
}
