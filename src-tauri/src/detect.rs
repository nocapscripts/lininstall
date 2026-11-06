//! System detection: distro, package manager, hardware and GPU generation.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use std::{env, fs};

pub const ALL_PMS: [&str; 12] = [
    "dnf", "apt", "pacman", "zypper", "apk", "xbps", "emerge", "yay", "paru", "nix", "brew", "flatpak",
];

#[derive(Clone, Serialize)]
pub struct Gpu {
    pub name: String,
    pub vendor: String,
    pub generation: Option<String>,
    pub branch: Option<String>,
}

/// Everything the UI and the task commands need to know about this machine.
/// An empty `pm` / `default_pm` means "no package manager found".
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ctx {
    pub pm: String,
    pub default_pm: String,
    pub available_pms: Vec<String>,
    pub all_pms: Vec<String>,
    pub pm_choice: String,
    pub fedora: bool,
    pub ubuntu_like: bool,
    pub distro: String,
    pub hostname: String,
    pub kernel: String,
    pub cpu: String,
    #[serde(rename = "ramGB")]
    pub ram_gb: String,
    pub session: String,
    pub desktop: String,
    pub secure_boot: String,
    pub nvidia_driver: Option<String>,
    pub gpus: Vec<Gpu>,
    pub nvidia: Option<Gpu>,
    pub has_lspci: bool,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("valid regex")
}

static GPU_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"^\S+\s+[^:]+\[\w+\]:\s*"));
static GPU_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\s*\[[0-9a-f]{4}:[0-9a-f]{4}\].*$"));
static GPU_LINE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)VGA compatible|3D controller|Display controller"));
static GEN_CODE: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b(GP|GM|GV|GK|GF|TU|GA|AD|GB)\d{2,3}"));
static PASCAL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)GTX\s*10\d\d|GT\s*10[34]0|TITAN X|TITAN Xp|Quadro P\d{3,4}|MX1[5-9]0")
});
static MAXWELL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)GTX\s*9\d\d|GTX\s*75\d|GTX\s*TITAN X|Quadro M\d{3,4}|MX1[1-4]0")
});
static KEPLER: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)GTX\s*[67]\d\d|GT\s*[67]\d\d|Quadro K"));
static TURING: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)RTX|GTX\s*16\d\d|Quadro RTX"));
static V_NVIDIA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bNVIDIA\b"));
static V_INTEL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bIntel\b"));
static V_AMD: LazyLock<Regex> = LazyLock::new(|| re(r"\bAMD\b|\bATI\b|Advanced Micro Devices"));

// ---------- PATH helpers ----------

/// PATH for child processes. GUI apps often get a trimmed PATH, so add the usual
/// system dirs and Linuxbrew locations.
pub fn extended_path() -> String {
    let mut dirs: Vec<String> = env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let home = env::var("HOME").unwrap_or_default();
    for d in [
        "/home/linuxbrew/.linuxbrew/bin".to_string(),
        format!("{home}/.linuxbrew/bin"),
    ] {
        if Path::new(&d).join("brew").exists() && !dirs.contains(&d) {
            dirs.insert(0, d);
        }
    }
    for d in ["/usr/local/bin", "/usr/bin", "/bin", "/usr/local/sbin", "/usr/sbin", "/sbin"] {
        if !dirs.iter().any(|x| x == d) {
            dirs.push(d.to_string());
        }
    }
    dirs.join(":")
}

pub fn has(bin: &str) -> bool {
    extended_path().split(':').any(|d| {
        fs::metadata(Path::new(d).join(bin))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// Run a program and return its stdout, or an empty string on any failure / after 8 seconds.
fn run(prog: &str, args: &[&str]) -> String {
    let spawned = Command::new(prog)
        .args(args)
        .env("PATH", extended_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return String::new();
    };
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                return if status.success() { out.trim().to_string() } else { String::new() };
            }
            Ok(None) if start.elapsed() > Duration::from_secs(8) => {
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => return String::new(),
        }
    }
}

// ---------- GPU ----------

fn nvidia_generation(name: &str) -> Option<(&'static str, &'static str)> {
    const PASCAL_G: (&str, &str) = ("Pascal", "580xx");
    const MAXWELL_G: (&str, &str) = ("Maxwell", "580xx");
    if let Some(c) = GEN_CODE.captures(name) {
        return match &c[1] {
            "GP" => Some(PASCAL_G),
            "GM" => Some(MAXWELL_G),
            "GV" => Some(("Volta", "580xx")),
            "GK" => Some(("Kepler", "470xx")),
            "GF" => Some(("Fermi", "390xx")),
            "TU" => Some(("Turing", "current")),
            "GA" => Some(("Ampere", "current")),
            "AD" => Some(("Ada Lovelace", "current")),
            "GB" => Some(("Blackwell", "current")),
            _ => None,
        };
    }
    // Fallback when lspci only shows the marketing name
    if PASCAL.is_match(name) {
        Some(PASCAL_G)
    } else if MAXWELL.is_match(name) {
        Some(MAXWELL_G)
    } else if KEPLER.is_match(name) {
        Some(("Kepler", "470xx"))
    } else if TURING.is_match(name) {
        Some(("Turing", "current"))
    } else {
        None
    }
}

fn parse_gpu(line: &str) -> Gpu {
    let name = GPU_PREFIX.replace(line, "").into_owned();
    let name = GPU_SUFFIX.replace(&name, "").into_owned();
    let vendor = if V_NVIDIA.is_match(&name) {
        "NVIDIA"
    } else if V_INTEL.is_match(&name) {
        "Intel"
    } else if V_AMD.is_match(&name) {
        "AMD"
    } else {
        "Other"
    };
    let (generation, branch) = if vendor == "NVIDIA" {
        match nvidia_generation(&name) {
            Some((g, b)) => (Some(g.to_string()), Some(b.to_string())),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    Gpu { name, vendor: vendor.to_string(), generation, branch }
}

// ---------- system ----------

fn os_release() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(text) = fs::read_to_string("/etc/os-release") {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if !k.is_empty() {
                    map.insert(k.to_string(), v.trim_matches('"').to_string());
                }
            }
        }
    }
    map
}

fn read_trim(path: &str) -> String {
    fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

fn cpu_info() -> String {
    let text = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let threads = text.lines().filter(|l| l.starts_with("processor")).count();
    let model = text
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string());
    match (model, threads) {
        (Some(m), t) if t > 0 => format!("{m} ({t} threads)"),
        (Some(m), _) => m,
        _ if threads > 0 => format!("CPU ({threads} threads)"),
        _ => "unknown".to_string(),
    }
}

fn ram_gb() -> String {
    let kb: f64 = fs::read_to_string("/proc/meminfo")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0.0);
    format!("{:.1}", kb / 1024.0 / 1024.0)
}

/// Apply the user's package manager choice ("auto" = the system default).
pub fn apply_choice(c: &mut Ctx, choice: &str) {
    c.pm_choice = if choice != "auto" && c.all_pms.iter().any(|p| p == choice) {
        choice.to_string()
    } else {
        "auto".to_string()
    };
    c.pm = if c.pm_choice == "auto" { c.default_pm.clone() } else { c.pm_choice.clone() };
}

pub fn detect() -> Ctx {
    let rel = os_release();
    let get = |k: &str| rel.get(k).cloned().unwrap_or_default();
    let id_like = format!("{} {}", get("ID"), get("ID_LIKE"));
    let is = |p: &str| re(&format!("(?i){p}")).is_match(&id_like);

    // [binary, name] - system managers first so they win the fallback
    let available: Vec<String> = [
        ("dnf", "dnf"), ("apt-get", "apt"), ("pacman", "pacman"), ("zypper", "zypper"),
        ("apk", "apk"), ("xbps-install", "xbps"), ("emerge", "emerge"),
        ("yay", "yay"), ("paru", "paru"), ("nix", "nix"), ("brew", "brew"),
        /* flatpak */
        ("flatpak", "flatpak"),
    ]
    .iter()
    .filter(|(bin, _)| has(bin))
    .map(|(_, name)| name.to_string())
    .collect();

    // The system's own package manager, based on the distro family
    let family = if is("fedora|rhel|centos|rocky|alma") { "dnf" }
        else if is("debian|ubuntu|mint|pop") { "apt" }
        else if is("arch|manjaro|endeavour|cachyos") { "pacman" }
        else if is("suse") { "zypper" }
        else if is("alpine") { "apk" }
        else if is("void") { "xbps" }
        else if is("gentoo") { "emerge" }
        else if is("nixos") { "nix" }
        else { "" };
    let default_pm = if !family.is_empty() && available.iter().any(|a| a == family) {
        family.to_string()
    } else {
        available.first().cloned().unwrap_or_default()
    };

    let gpus: Vec<Gpu> = run("lspci", &["-nn"])
        .lines()
        .filter(|l| GPU_LINE.is_match(l))
        .map(parse_gpu)
        .collect();
    let nvidia = gpus.iter().find(|g| g.vendor == "NVIDIA").cloned();

    let hostname = {
        let h = read_trim("/proc/sys/kernel/hostname");
        if h.is_empty() { read_trim("/etc/hostname") } else { h }
    };
    let secure_boot = run("mokutil", &["--sb-state"])
        .lines()
        .next()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown (mokutil not installed)".to_string());
    let nvidia_driver = run(
        "nvidia-smi",
        &["--query-gpu=driver_version", "--format=csv,noheader"],
    )
    .lines()
    .next()
    .map(str::to_string)
    .filter(|s| !s.is_empty());
    let unknown = || "unknown".to_string();

    let mut ctx = Ctx {
        pm: default_pm.clone(),
        default_pm,
        available_pms: available,
        all_pms: ALL_PMS.iter().map(|s| s.to_string()).collect(),
        pm_choice: "auto".to_string(),
        fedora: is("fedora"),
        ubuntu_like: is("ubuntu"),
        distro: rel.get("PRETTY_NAME").cloned().unwrap_or_else(|| "Linux".to_string()),
        hostname,
        kernel: read_trim("/proc/sys/kernel/osrelease"),
        cpu: cpu_info(),
        ram_gb: ram_gb(),
        session: env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| unknown()),
        desktop: env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| unknown()),
        secure_boot,
        nvidia_driver,
        gpus,
        nvidia,
        has_lspci: has("lspci"),
    };
    apply_choice(&mut ctx, "auto");
    ctx
}
