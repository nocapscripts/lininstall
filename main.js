const { app, BrowserWindow, ipcMain, clipboard } = require("electron");
const { spawn, exec } = require("child_process");
const path = require("path");
const fs = require("fs");
const categories = require("./tasks");
const { detect } = require("./detect");

const settingsFile = () => path.join(app.getPath("userData"), "settings.json");
const loadSettings = () => { try { return JSON.parse(fs.readFileSync(settingsFile(), "utf8")); } catch { return {}; } };
const saveSettings = (s) => { try { fs.writeFileSync(settingsFile(), JSON.stringify(s)); } catch {} };

let pmChoice = "auto";
// Detect the system, then apply the user's package manager choice ("auto" = system default)
const detectCtx = () => {
  const c = detect();
  c.pmChoice = pmChoice !== "auto" && c.allPms.includes(pmChoice) ? pmChoice : "auto";
  c.pm = c.pmChoice === "auto" ? c.defaultPm : c.pmChoice;
  return c;
};
let ctx = null;
let win;
let running = false;

// Package managers that work in user space (no root); the rest run through pkexec
const USER_ONLY_PMS = ["brew", "nix"];
const USER_CMD_RE = /^\s*(brew|nix|yay|paru)\b/;
const resolveCmd = (task) => {
  if (task.sys && USER_ONLY_PMS.includes(ctx.pm)) return undefined;
  try { return typeof task.cmd === "function" ? task.cmd(ctx) : task.cmd; } catch { return undefined; }
};
const isRoot = (task, cmd) => task.root !== false && !USER_CMD_RE.test(cmd || "");

function createWindow() {
  win = new BrowserWindow({
    width: 1180, height: 760, minWidth: 900, minHeight: 600,
    backgroundColor: "#0e1014", title: "LinInstall", icon: path.join(__dirname, "build", "icon.png"),
    webPreferences: { preload: path.join(__dirname, "preload.js"), contextIsolation: true },
    
  });
  win.setMenuBarVisibility(false);
  win.loadFile("index.html");
}

const payload = () => ({
  info: ctx,
  categories: categories.map((c) => ({
    id: c.id, name: c.name, icon: c.icon,
    tasks: c.tasks.map((t) => {
      const cmd = resolveCmd(t);
      return { id: t.id, name: t.name, desc: t.desc, group: t.group || "", root: isRoot(t, cmd), reboot: !!t.reboot, danger: !!t.danger, cmd: cmd || null };
    }),
  })),
});

ipcMain.handle("get-version", () => app.getVersion());
ipcMain.handle("get-data", () => payload());
ipcMain.handle("refresh", () => { ctx = detectCtx(); return payload(); });
ipcMain.handle("set-pm", (_e, choice) => {
  pmChoice = choice === "auto" || ctx.allPms.includes(choice) ? choice : "auto";
  saveSettings({ ...loadSettings(), pmChoice });
  ctx = detectCtx();
  return payload();
});
ipcMain.handle("copy", (_e, text) => clipboard.writeText(text));

ipcMain.handle("run-task", (_e, id) => new Promise((resolve) => {
  if (running) return resolve({ code: -1, busy: true });
  const task = categories.flatMap((c) => c.tasks).find((t) => t.id === id);
  const cmd = task && resolveCmd(task);
  const send = (t) => win && !win.isDestroyed() && win.webContents.send("log", t);
  if (!cmd) { send("\nThis task is not supported on your distro.\n"); return resolve({ code: 1 }); }

  running = true;
  send(`\n━━ ${task.name} ━━\n`);
  const child = isRoot(task, cmd)
    ? spawn("pkexec", ["bash", "-c", cmd])   // graphical password prompt
    : spawn("bash", ["-c", cmd]);
  child.stdout.on("data", (d) => send(d.toString()));
  child.stderr.on("data", (d) => send(d.toString()));
  child.on("error", (err) => { send(`Error: ${err.message}\n`); running = false; resolve({ code: 1 }); });
  child.on("close", (code) => {
    running = false;
    send(`\n[${code === 0 ? "✔ finished" : "✖ exited with code " + code}]\n`);
    if (code === 0 && task.reboot) send("[↻ A reboot is recommended]\n");
    resolve({ code, reboot: code === 0 && !!task.reboot });
  });
}));

// ---------- Remove installed packages ----------
const sh = (cmd) => new Promise((res) => exec(cmd, { maxBuffer: 1024 * 1024 * 20 }, (_e, out) => res(out || "")));
const PKG_RE = /^[A-Za-z0-9][A-Za-z0-9+._:@\/-]*$/;
// Never offered for removal (use the GPU Drivers page for NVIDIA)
const PROTECTED = /^(kernel.*|linux|linux-.*|systemd.*|glibc.*|bash|sudo|polkit.*|dnf.*|rpm.*|apt.*|dpkg|pacman|zypper|coreutils.*|NetworkManager.*|grub.*|shim.*|dracut.*|mutter|gnome-shell|plasma-.*|gdm|sddm|lightdm|cinnamon.*|xorg-x11-server.*|selinux.*|firewalld|nvidia.*|akmod.*|xorg-x11-drv.*|apk-tools.*|xbps.*|portage.*|busybox|musl.*|openrc.*|nix|brew|lininstall)$/i;

const LIST = {
  dnf: "dnf repoquery --userinstalled --queryformat '%{name}\\n' 2>/dev/null | sort -u",
  apt: "apt-mark showmanual | sort -u",
  pacman: "pacman -Qqe | sort -u",
  yay: "pacman -Qqe | sort -u",
  paru: "pacman -Qqe | sort -u",
  zypper: "rpm -qa --qf '%{NAME}\\n' | sort -u",
  apk: "sed 's/[<>=~].*//' /etc/apk/world | sort -u",
  xbps: "xbps-query -m | sed 's/-[^-]*$//' | sort -u",
  emerge: "sed 's/[<>=~].*//' /var/lib/portage/world | sort -u",
  brew: "brew leaves",
};

ipcMain.handle("list-packages", async () => {
  let native = (await sh(LIST[ctx.pm] || "true")).split("\n").map((s) => s.trim()).filter(Boolean);
  if (!native.length && ["dnf", "zypper"].includes(ctx.pm))
    native = (await sh("rpm -qa --qf '%{NAME}\\n' | sort -u")).split("\n").map((s) => s.trim()).filter(Boolean);
  native = native.filter((n) => PKG_RE.test(n) && !PROTECTED.test(n));
  const flatpak = (await sh("flatpak list --app --columns=application,name 2>/dev/null"))
    .split("\n").filter(Boolean)
    .map((l) => { const [id, name] = l.split("\t"); return { id: (id || "").trim(), name: (name || id || "").trim() }; })
    .filter((f) => PKG_RE.test(f.id));
  return { native, flatpak };
});

function runCommand(cmd, root, label) {
  return new Promise((resolve) => {
    if (running) return resolve({ code: -1, busy: true });
    const send = (t) => win && !win.isDestroyed() && win.webContents.send("log", t);
    running = true;
    send(`\n━━ ${label} ━━\n$ ${cmd}\n`);
    const child = root ? spawn("pkexec", ["bash", "-c", cmd]) : spawn("bash", ["-c", cmd]);
    child.stdout.on("data", (d) => send(d.toString()));
    child.stderr.on("data", (d) => send(d.toString()));
    child.on("error", (err) => { send(`Error: ${err.message}\n`); running = false; resolve({ code: 1 }); });
    child.on("close", (code) => { running = false; send(`\n[${code === 0 ? "✔ finished" : "✖ exited with code " + code}]\n`); resolve({ code }); });
  });
}

ipcMain.handle("remove-packages", async (_e, sel, dry) => {
  const native = (sel.native || []).filter((n) => PKG_RE.test(n) && !PROTECTED.test(n));
  const flatpak = (sel.flatpak || []).filter((f) => PKG_RE.test(f));
  const l = native.join(" ");
  const real = {
    dnf: `dnf remove -y ${l}`,
    apt: `export DEBIAN_FRONTEND=noninteractive; apt-get purge -y ${l} && apt-get autoremove -y`,
    pacman: `pacman -Rns --noconfirm ${l}`,
    zypper: `zypper --non-interactive remove ${l}`,
    apk: `apk del ${l}`,
    xbps: `xbps-remove -Ry ${l}`,
    emerge: `emerge --ask=n --depclean ${l}`,
    brew: `brew uninstall ${l}`,
    yay: `yay -Rns --noconfirm --sudo pkexec ${l}`,
    paru: `paru -Rns --noconfirm --sudo pkexec ${l}`,
  }[ctx.pm];
  const preview = {
    dnf: `dnf remove --assumeno ${l}`,
    apt: `apt-get -s purge ${l}`,
    pacman: `pacman -Rnsp ${l}`,
    zypper: `zypper --non-interactive remove --dry-run ${l}`,
    apk: `apk del -s ${l}`,
    xbps: `xbps-remove -Rn ${l}`,
    emerge: `emerge -pc ${l}`,
    brew: `echo "Would uninstall: ${l}"`,
    yay: `pacman -Rnsp ${l}`,
    paru: `pacman -Rnsp ${l}`,
  }[ctx.pm];
  if (native.length && !real) {
    if (win && !win.isDestroyed()) win.webContents.send("log", `\nRemoving packages is not supported for ${ctx.pm}.\n`);
    return { code: 1 };
  }

  if (dry) {
    if (!native.length) return { code: 0 };
    return runCommand(`${preview}; true`, false, "Preview: what would be removed");
  }
  let r = { code: 0 };
  if (native.length) r = await runCommand(real, !USER_CMD_RE.test(real), "Remove packages");
  if (flatpak.length && r.code === 0) r = await runCommand(`flatpak uninstall -y ${flatpak.join(" ")}`, false, "Remove Flatpak apps");
  return r;
});

app.whenReady().then(() => {
  pmChoice = loadSettings().pmChoice || "auto";
  ctx = detectCtx();
  createWindow();
});
app.on("window-all-closed", () => app.quit());
