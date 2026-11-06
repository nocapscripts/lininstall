<div align="center">

<img src="build/icons/256x256.png" alt="LinInstall logo" width="128" />

# LinInstall

**A modern Linux utility with a built-in GPU driver manager, built with Tauri, Rust and Tailwind CSS.**
Update your system, install apps, fix NVIDIA drivers (even Pascal), harden security and tweak performance, all from one clean window. Designed for smoothiness.

[![License: MIT](https://img.shields.io/badge/License-MIT-7c8cff.svg?style=for-the-badge)](LICENSE)
[![Tauri](https://img.shields.io/badge/Tauri-2-24c8db?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-logic-dea584?style=for-the-badge&logo=rust&logoColor=black)](https://www.rust-lang.org/)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind-3-38bdf8?style=for-the-badge&logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![Fedora](https://img.shields.io/badge/Fedora-supported-51a2da?style=for-the-badge&logo=fedora&logoColor=white)](https://fedoraproject.org/)
[![Platform](https://img.shields.io/badge/Linux-x64-fcc624?style=for-the-badge&logo=linux&logoColor=black)](#-supported-distros)

[Features](#-features) · [Install](#-install) · [GPU drivers](#-gpu-drivers) · [Build](#-build-from-source) · [Add your own tasks](#-add-your-own-tasks) · [FAQ](#-faq)

<br />

<img src="docs/screenshot.png" alt="LinInstall screenshot" width="860" />

</div>

---

## ✨ Features

| | |
|---|---|
| 🏠 **Overview** | Distro, kernel, CPU, RAM, session type (X11/Wayland), Secure Boot state and every detected GPU with its generation |
| 🎮 **GPU drivers** | Detects your GPU and installs the right NVIDIA driver branch in one click, including **legacy Pascal / Maxwell / Volta** cards |
| ⚙️ **System** | Updates, cache cleanup, firmware updates, RPM Fusion, Flathub, codecs, build tools, SSD TRIM, swappiness |
| 📦 **Applications** | Browsers (Firefox, Chromium, Brave, Zen), dev tools (Git, Node, Python, Docker, VS Code, Zed), media, gaming and terminal tools |
| 🗑️ **Remove apps** | Tick the packages you installed and remove them, with a dry-run **Preview** first |
| 🛡️ **Security** | Firewall, Fail2ban, automatic updates, ClamAV, SELinux status, open-port overview |
| 🧰 **Utilities** | System info, SMART disk health, boot time analysis, network info, cache cleanup |
| 🔍 **Search** | One search box across every task |
| 👀 **Safe by design** | Every action shows the **exact command** and whether it needs root before it runs |

## 🧭 How it works

1. Pick a task in the sidebar (or search for it).
2. A dialog shows the exact shell command, and whether it runs as root or as your user.
3. Confirm, and the output streams live into the log panel.
4. Tasks that need root use `pkexec`, so you get the normal graphical password prompt. The app itself never runs as root.

## 🖥️ GPU drivers

LinInstall reads your GPU from `lspci` and maps the chip codename to the driver branch that actually supports it.

| Generation | Example cards | Driver branch |
|---|---|---|
| Turing, Ampere, Ada, Blackwell | GTX 16xx, RTX 20/30/40/50 | **current** |
| Maxwell, **Pascal**, Volta | GTX 750, GTX 9xx, **GTX 10xx**, Titan X/Xp, Quadro P/M | **580xx** (legacy) |
| Kepler | GTX 600/700 | 470xx (legacy) |
| Fermi | GTX 400/500 | 390xx (legacy, Fedora) |

> [!NOTE]
> NVIDIA's 580 series is the last branch that supports Maxwell, Pascal and Volta. Newer drivers dropped them, so those cards need the legacy packages.

**On Fedora** the installer enables RPM Fusion Free and Nonfree, removes any conflicting NVIDIA packages, installs `akmod-nvidia-580xx` with the matching `xorg-x11-drv-nvidia-580xx` packages (or the current / 470xx / 390xx equivalents), builds the kernel module with `akmods` and rebuilds the initramfs. Then reboot.

Extra GPU tools: nvidia-drm modeset, forced akmods rebuild, Secure Boot key-enrollment help, driver removal, AMD and Intel graphics stacks, Vulkan tools.

> [!WARNING]
> With **Secure Boot enabled**, NVIDIA kernel modules need an enrolled signing key. The app shows the steps (`kmodgenca` + `mokutil --import`), but the password prompt has to happen in a terminal. Alternatively, disable Secure Boot in your firmware settings.

## 🐧 Supported distros

| Distro family | Package manager | Status |
|---|---|---|
| Fedora | `dnf` | ✅ Best supported (developed and tested on Fedora 44, X11) |
| Ubuntu, Debian and derivatives | `apt` | 🟡 Supported (Pascal relies on `ubuntu-drivers` picking the right version) |
| Arch and derivatives | `pacman` | 🟡 Supported (legacy NVIDIA needs `yay` or `paru` for the AUR) |
| openSUSE | `zypper` | 🟠 Partial (many tasks work, NVIDIA install is not supported) |

Tasks that don't exist for your distro are greyed out with a "not supported" label instead of failing.

## 📥 Install

Grab a package from the [**Releases**](../../releases) page.

```bash
# Fedora / RHEL-based
sudo dnf install ./LinInstall-<ver>.rpm

# Debian / Ubuntu
sudo apt install ./LinInstall_<ver>_amd64.deb

# Any distro (AppImage)
chmod +x LinInstall_<ver>_amd64.AppImage
./LinInstall_<ver>_amd64.AppImage
```

**Requirements:** `polkit` (for `pkexec`) and `pciutils` (for `lspci`). The rpm and deb packages pull these in automatically. The app uses the system WebKitGTK, so installs are small and start fast.

## 🔨 Build from source

```bash
./build.sh rpm            # or: deb | appimage | all   (add --install to install the rpm)

# development
npm install
npm run tauri dev
```

Output goes to `src-tauri/target/release/bundle/`.

<details>
<summary><b>Build prerequisites</b></summary>

```bash
# Fedora
sudo dnf install nodejs npm rpm-build webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel gcc gcc-c++ make
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Debian / Ubuntu
sudo apt install nodejs npm build-essential curl wget file libwebkit2gtk-4.1-dev \
  libssl-dev libxdo-dev libayatana-appindicator3-dev librsvg2-dev
```

</details>

## 🏗️ Project layout

| Path | What it does |
|---|---|
| `src-tauri/src/detect.rs` | Distro, package manager, hardware and GPU generation detection |
| `src-tauri/src/tasks.rs` | The task catalog (every command the app can run) |
| `src-tauri/src/commands.rs` | Tauri commands: run tasks with live output, list and remove packages |
| `src/index.html`, `src/main.js` | The UI (plain JS, no bundler) |
| `styles/input.css`, `tailwind.config.js` | Tailwind CSS, built into `src/assets/tailwind.css` |

## 🧩 Add your own tasks

All tasks live in [`src-tauri/src/tasks.rs`](src-tauri/src/tasks.rs). A task is one line:

```rust
task("htop", "htop", "Process viewer", "Terminal tools", inst("htop", &[]))
```

| Part | Meaning |
|---|---|
| `id`, name, description | Identifier, title and description shown in the UI |
| group | Heading the task appears under |
| `.user()` | Run as your user (default is root through `pkexec`) |
| `.reboot()` | Show a "reboot recommended" note after success |
| `.danger()` | Mark the task as "careful" and style the button red |
| `.sys()` | System-level task: hidden on user-space managers (brew, nix) |
| last argument | The command builder (see below). Returning `None` marks the task unsupported |

Command builders:

```rust
inst("pkg", &[])                              // install a package with the detected package manager
inst("pkg", &[("pacman", Some("other-name"))]) // per-distro package names (None = unsupported)
by(vec![("dnf", "...".into()), ("apt", "...".into())]) // a different command per package manager
fixed("echo hello")                           // the same command everywhere
func(|ctx| Some(format!("echo {}", ctx.pm)))  // full control, with access to the detected system
flatpak_cmd("org.example.App")                // install a Flatpak from Flathub as your user
```


## 🔐 Security notes

- Every command is shown **before** it runs. Nothing executes silently.
- The UI runs with a strict Content Security Policy. It can only ask Rust to run a task by **id** from the built-in catalog; it never sends a command to execute.
- Root commands go through `pkexec`, so authentication is handled by polkit and not by the app.
- Package names for the remove feature are validated, and core system packages (kernel, systemd, glibc, your desktop environment, the package manager, NVIDIA drivers) are hidden from the removal list.
- Several tasks download and run vendor install scripts (Brave, Zed) or add third-party repos (VS Code). Read the command in the dialog if you want to check it first.

## ❓ FAQ

<details>
<summary><b>My GPU is detected as the wrong vendor or generation</b></summary>

Open an issue with the output of `lspci -nn | grep -Ei 'vga|3d|display'` and your distro version.

</details>

<details>
<summary><b>The NVIDIA driver installed but I'm still on nouveau</b></summary>

On Fedora the kernel module builds in the background and takes a few minutes. Wait for it to finish (`modinfo -F version nvidia` should print a version), then reboot. If Secure Boot is on, the module also needs an enrolled key.

</details>

<details>
<summary><b>The icon doesn't show after installing the rpm</b></summary>

Check that the package contains sized icons: `rpm -qlp dist/*.rpm | grep icons`. Then refresh the cache: `sudo gtk-update-icon-cache -f /usr/share/icons/hicolor`.

</details>

<details>
<summary><b>Is this affiliated with linutil?</b></summary>

No. LinInstall is an independent project inspired by [ChrisTitusTech/linutil](https://github.com/ChrisTitusTech/linutil) and shares no code with it.

</details>

## 🗺️ Roadmap

- [ ] Custom task profiles and one-click "setup presets"
- [ ] Per-distro task testing on Ubuntu and Arch
- [ ] Wayland-specific tweaks
- [ ] Update notifications for the app itself

## 🤝 Contributing

Issues and pull requests are welcome. To add a task, edit `src-tauri/src/tasks.rs`, run it with `npm run tauri dev`, and please say which distro you tested it on.

Do not create any similar phishing and scam related program from this source!

This project should be for linux community made by love and enthusiasm


## 💜 Community & responsible use
 
LinInstall is made for the Linux community, with love and enthusiasm.
 
Please **do not** use this source to build phishing tools, scams, malware or anything else meant to deceive or harm people. Contributions with that kind of purpose will not be accepted.

## 📄 License

Released under the [MIT License](LICENSE). Copyright © 2026 NoCapScripts.

<div align="center">
<sub>Made by NoCapScripts </sub>
</div>
