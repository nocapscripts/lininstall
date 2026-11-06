//! The task catalog. Each task turns the detected system (`Ctx`) into a shell command,
//! or `None` when the task is not supported on this distro.
//!
//! Add your own task by appending a `task(...)` line to the matching category below, e.g.
//!   task("htop", "htop", "Process viewer", "Terminal tools", inst("htop", &[]))

use crate::detect::Ctx;
use std::sync::LazyLock;

pub type CmdFn = Box<dyn Fn(&Ctx) -> Option<String> + Send + Sync>;

pub struct Task {
    pub id: &'static str,
    pub name: String,
    pub desc: String,
    pub group: &'static str,
    /// System-level task: hidden on user-space package managers (brew, nix)
    pub sys: bool,
    /// Run through pkexec (default). Use `.user()` for user-level commands.
    pub root: bool,
    pub reboot: bool,
    pub danger: bool,
    pub cmd: CmdFn,
}

impl Task {
    fn user(mut self) -> Self { self.root = false; self }
    fn reboot(mut self) -> Self { self.reboot = true; self }
    fn danger(mut self) -> Self { self.danger = true; self }
    fn sys(mut self) -> Self { self.sys = true; self }
}

pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub tasks: Vec<Task>,
}

pub static CATALOG: LazyLock<Vec<Category>> = LazyLock::new(catalog);

// ---------- helpers ----------

fn task(id: &'static str, name: &str, desc: &str, group: &'static str, cmd: CmdFn) -> Task {
    Task {
        id, name: name.to_string(), desc: desc.to_string(), group,
        sys: false, root: true, reboot: false, danger: false, cmd,
    }
}

fn func<F>(f: F) -> CmdFn
where
    F: Fn(&Ctx) -> Option<String> + Send + Sync + 'static,
{
    Box::new(f)
}

/// The same command on every distro.
fn fixed(s: &str) -> CmdFn {
    let s = s.to_string();
    Box::new(move |_| Some(s.clone()))
}

/// A different command per package manager; unlisted managers are unsupported.
fn by(pairs: Vec<(&'static str, String)>) -> CmdFn {
    Box::new(move |c| pairs.iter().find(|e| e.0 == c.pm.as_str()).map(|e| e.1.clone()))
}

/// Install a package with the detected manager. `over` overrides the package list per
/// manager (`None` = unsupported there).
fn inst(all: &'static str, over: &'static [(&'static str, Option<&'static str>)]) -> CmdFn {
    func(move |c| {
        let l = match over.iter().find(|e| e.0 == c.pm.as_str()) {
            Some(e) => e.1,
            None => Some(all),
        };
        install_cmd(&c.pm, l?)
    })
}

pub fn install_cmd(pm: &str, l: &str) -> Option<String> {
    Some(match pm {
        "apt" => format!("export DEBIAN_FRONTEND=noninteractive; apt-get install -y {l}"),
        "pacman" => format!("pacman -S --noconfirm --needed {l}"),
        "dnf" => format!("dnf install -y {l}"),
        "zypper" => format!("zypper --non-interactive install {l}"),
        "apk" => format!("apk add {l}"),
        "xbps" => format!("xbps-install -Sy {l}"),
        "emerge" => format!("emerge --ask=n --noreplace {l}"),
        "brew" => format!("brew install {l}"),
        "nix" => format!(
            "nix profile install {}",
            l.split(' ').map(|p| format!("nixpkgs#{p}")).collect::<Vec<_>>().join(" ")
        ),
        "yay" => format!("yay -S --noconfirm --needed --sudo pkexec {l}"),
        "paru" => format!("paru -S --noconfirm --needed --sudo pkexec {l}"),
        _ => return None,
    })
}

/// `install_cmd` for a manager that is known at compile time.
fn ic(pm: &str, l: &str) -> String {
    install_cmd(pm, l).unwrap_or_default()
}

fn lines(parts: &[&str]) -> String {
    parts.join("\n")
}

fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" && ")
}

fn flatpak_cmd(id: &str) -> String {
    format!(
        "flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install --user -y flathub {id}"
    )
}

const RPMFUSION: &str = "dnf install -y https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm";

fn rebuild_initramfs(pm: &str) -> &'static str {
    match pm {
        "dnf" | "zypper" => "dracut --force",
        "apt" => "update-initramfs -u",
        "pacman" => "mkinitcpio -P",
        _ => "true",
    }
}

// ---------- NVIDIA ----------

/// branch: "current" (Turing+), "580xx" (Maxwell/Pascal/Volta), "470xx" (Kepler), "390xx" (Fermi)
fn nvidia(branch: &'static str) -> CmdFn {
    func(move |c| nvidia_cmd(c, branch))
}

fn nvidia_cmd(c: &Ctx, branch: &str) -> Option<String> {
    if c.pm == "dnf" && c.fedora {
        let s = if branch == "current" { String::new() } else { format!("-{branch}") };
        let install = format!(
            "dnf install -y kernel-devel akmod-nvidia{s} xorg-x11-drv-nvidia{s} xorg-x11-drv-nvidia{s}-cuda"
        );
        return Some(lines(&[
            "set -e",
            RPMFUSION,
            "dnf remove -y 'xorg-x11-drv-nvidia*' 'akmod-nvidia*' || true",
            &install,
            "echo 'Building kernel module (this can take several minutes)...'",
            "akmods --force",
            "dracut --force",
            "echo; echo 'Done. Reboot to load the driver.'",
        ]));
    }
    if c.pm == "apt" {
        if branch == "390xx" {
            return None;
        }
        let legacy = match branch {
            "580xx" => Some("nvidia-driver-580"),
            "470xx" => Some("nvidia-driver-470"),
            _ => None,
        };
        let line = match legacy {
            Some(l) => format!("  apt-get install -y {l} || ubuntu-drivers install"),
            None => "  ubuntu-drivers install".to_string(),
        };
        return Some(lines(&[
            "export DEBIAN_FRONTEND=noninteractive",
            "apt-get update",
            "if command -v ubuntu-drivers >/dev/null 2>&1; then",
            &line,
            "else",
            "  apt-get install -y nvidia-driver firmware-misc-nonfree",
            "fi",
            "echo; echo 'Done. Reboot to load the driver.'",
        ]));
    }
    if c.pm == "pacman" && branch == "current" {
        return Some("pacman -S --noconfirm --needed linux-headers nvidia-open-dkms nvidia-utils nvidia-settings && echo 'Done. Reboot.'".to_string());
    }
    // Arch legacy drivers live in the AUR, see arch_legacy()
    None
}

/// Arch legacy (AUR): runs as the normal user via yay/paru
fn arch_legacy(c: &Ctx, branch: &str) -> Option<String> {
    if c.pm != "pacman" {
        return None;
    }
    let pkgs = if branch == "580xx" {
        "nvidia-580xx-dkms nvidia-580xx-utils"
    } else {
        "nvidia-470xx-dkms nvidia-470xx-utils"
    };
    let head = "H=$(command -v yay || command -v paru) || { echo 'Install yay or paru first (AUR helper needed).'; exit 1; }; sudo pacman -S --needed --noconfirm linux-headers; $H -S --needed ";
    Some(format!("{head}{pkgs}"))
}

fn nv_auto(c: &Ctx) -> Option<String> {
    let Some(g) = &c.nvidia else {
        return Some("echo 'No NVIDIA GPU detected (is pciutils/lspci installed?).'".to_string());
    };
    let Some(branch) = &g.branch else {
        return Some("echo 'Could not determine the NVIDIA generation. Pick a driver manually.'".to_string());
    };
    if c.pm == "pacman" && branch != "current" {
        return arch_legacy(c, branch);
    }
    nvidia_cmd(c, branch)
}

// ---------- Web tools helpers ----------

/// Enable + start a service on systemd, OpenRC (Alpine/Gentoo) or runit (Void)
fn svc(n: &str) -> String {
    format!("if command -v systemctl >/dev/null 2>&1; then systemctl enable --now {n}; elif command -v rc-update >/dev/null 2>&1; then rc-update add {n} default && rc-service {n} start; elif [ -d /etc/sv/{n} ]; then ln -sf /etc/sv/{n} /var/service/{n}; fi")
}

const INIT_DB: &str = "[ -d /var/lib/mysql/mysql ] || mariadb-install-db --user=mysql --basedir=/usr --datadir=/var/lib/mysql";

/// (packages, setup before start, service name)
fn db(pm: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match pm {
        "dnf" | "apt" => ("mariadb-server", "", "mariadb"),
        "zypper" => ("mariadb", "", "mariadb"),
        "pacman" | "yay" | "paru" | "xbps" => ("mariadb", INIT_DB, "mariadb"),
        "apk" => ("mariadb mariadb-client", INIT_DB, "mariadb"),
        "emerge" => ("dev-db/mariadb", INIT_DB, "mysql"),
        _ => return None,
    })
}

fn php_pkgs(pm: &str) -> Option<&'static str> {
    Some(match pm {
        "dnf" => "php php-mysqlnd php-fpm",
        "apt" => "php php-mysql php-fpm",
        "zypper" => "php8 php8-mysql php8-fpm",
        "pacman" | "yay" | "paru" => "php php-fpm",
        "apk" => "php83 php83-fpm php83-mysqlnd php83-pdo_mysql php83-mysqli",
        "xbps" => "php php-fpm php-mysql",
        "emerge" => "dev-lang/php",
        _ => return None,
    })
}

fn php_setup(pm: &str) -> &'static str {
    match pm {
        "zypper" => "cp -n /etc/php8/fpm/php-fpm.conf.default /etc/php8/fpm/php-fpm.conf; cp -n /etc/php8/fpm/php-fpm.d/www.conf.default /etc/php8/fpm/php-fpm.d/www.conf",
        "pacman" | "yay" | "paru" => "sed -i 's/^;extension=mysqli/extension=mysqli/; s/^;extension=pdo_mysql/extension=pdo_mysql/' /etc/php/php.ini",
        _ => "",
    }
}

fn php_svc(pm: &str) -> String {
    match pm {
        "apt" => "systemctl enable --now $(systemctl list-unit-files --no-legend 'php*-fpm.service' | awk 'NR==1{print $1}')".to_string(),
        "apk" => svc("php-fpm83"),
        _ => svc("php-fpm"),
    }
}

fn nginx_pkg(pm: &str) -> &'static str {
    if pm == "emerge" { "www-servers/nginx" } else { "nginx" }
}

// MongoDB Community series - see https://www.mongodb.com/try/download/community
const MONGO_VER: &str = "9.0";

fn mongo_dnf() -> String {
    let v = MONGO_VER;
    format!(
        r#"set -e
V=$(rpm -E %rhel); case "$V" in 8|9|10) ;; *) V=9 ;; esac   # Fedora uses the RHEL 9 build
rpm --import https://pgp.mongodb.com/server-{v}.asc
cat > /etc/yum.repos.d/mongodb-org-{v}.repo <<EOF
[mongodb-org-{v}]
name=MongoDB Repository
baseurl=https://repo.mongodb.org/yum/redhat/$V/mongodb-org/{v}/$(uname -m)/
gpgcheck=1
enabled=1
gpgkey=https://pgp.mongodb.com/server-{v}.asc
EOF
dnf install -y mongodb-org"#
    )
}

fn mongo_apt() -> String {
    let v = MONGO_VER;
    format!(
        r#"set -e
export DEBIAN_FRONTEND=noninteractive
apt-get update && apt-get install -y gnupg curl ca-certificates
. /etc/os-release
CN=$UBUNTU_CODENAME; [ -n "$CN" ] || CN=$VERSION_CODENAME
case "$ID $ID_LIKE" in
  *ubuntu*) set -- ubuntu $CN multiverse ;;
  *) set -- debian $CN main ;;
esac
curl -fsSL https://pgp.mongodb.com/server-{v}.asc | gpg --dearmor --yes -o /usr/share/keyrings/mongodb-server-{v}.gpg
echo "deb [ signed-by=/usr/share/keyrings/mongodb-server-{v}.gpg ] https://repo.mongodb.org/apt/$1 $2/mongodb-org/{v} $3" > /etc/apt/sources.list.d/mongodb-org-{v}.list
apt-get update && apt-get install -y mongodb-org"#
    )
}

fn mongo_zypper() -> String {
    let v = MONGO_VER;
    format!(
        r#"set -e
. /etc/os-release
V=$(echo "$VERSION_ID" | cut -d. -f1); case "$V" in 15|16) ;; *) V=15 ;; esac
rpm --import https://pgp.mongodb.com/server-{v}.asc
zypper --non-interactive removerepo mongodb-org-{v} 2>/dev/null || true
zypper --non-interactive addrepo --gpgcheck "https://repo.mongodb.org/zypper/suse/$V/mongodb-org/{v}/$(uname -m)/" mongodb-org-{v}
zypper --non-interactive --gpg-auto-import-keys refresh
zypper --non-interactive install mongodb-org"#
    )
}

const COMPASS_VER: &str = "1.53.2";
const X64_ONLY: &str = "[ \"$(uname -m)\" = x86_64 ] || { echo 'MongoDB Compass is only available for x86_64.'; exit 1; }";

fn compass(c: &Ctx) -> Option<String> {
    let base = "https://downloads.mongodb.com/compass";
    let v = COMPASS_VER;
    let rpm = format!("{base}/mongodb-compass-{v}.x86_64.rpm");
    let deb = format!("{base}/mongodb-compass_{v}_amd64.deb");
    Some(match c.pm.as_str() {
        "dnf" => lines(&["set -e", X64_ONLY, &format!("dnf install -y {rpm}")]),
        "zypper" => lines(&["set -e", X64_ONLY, &format!("zypper --non-interactive --no-gpg-checks install {rpm}")]),
        "apt" => lines(&[
            "set -e",
            "export DEBIAN_FRONTEND=noninteractive",
            X64_ONLY,
            "apt-get update && apt-get install -y curl ca-certificates",
            &format!("curl -fSL -o /tmp/mongodb-compass.deb {deb}"),
            "apt-get install -y /tmp/mongodb-compass.deb",
            "rm -f /tmp/mongodb-compass.deb",
        ]),
        "yay" => ic("yay", "mongodb-compass"),
        "paru" => ic("paru", "mongodb-compass"),
        _ => return None,
    })
}

/// Estonian ID card stack: base packages + service, then the browser extension helper.
fn digidoc(pm: &str, base: &str, second: &str, msg: &str) -> String {
    format!(
        r#"{} && sudo systemctl enable --now pcscd.socket; {} || echo "{}""#,
        ic(pm, base), ic(pm, second), msg
    )
}

/// Repo-based install on apt: key + source list + install.
fn apt_repo(key_url: &str, keyring: &str, list_line: &str, list_file: &str, pkg: &str) -> String {
    let key = format!("wget -qO- {key_url} | gpg --dearmor > /usr/share/keyrings/{keyring}");
    let list = format!("echo '{list_line}' > /etc/apt/sources.list.d/{list_file}");
    let install = format!("apt-get update && apt-get install -y {pkg}");
    lines(&[
        "export DEBIAN_FRONTEND=noninteractive",
        "apt-get install -y wget gpg",
        &key,
        &list,
        &install,
    ])
}

// ---------- catalog ----------

pub fn catalog() -> Vec<Category> {
    vec![system(), gpu(), apps(), security(), utils()]
}

fn system() -> Category {
    let update = by(vec![
        ("dnf", "dnf upgrade -y --refresh".into()),
        ("apt", "export DEBIAN_FRONTEND=noninteractive; apt-get update && apt-get upgrade -y".into()),
        ("pacman", "pacman -Syu --noconfirm".into()),
        ("zypper", "zypper --non-interactive update".into()),
        ("apk", "apk update && apk upgrade".into()),
        ("xbps", "xbps-install -Syu".into()),
        ("emerge", "emerge --sync && emerge --ask=n -uDN @world".into()),
        ("brew", "brew update && brew upgrade".into()),
        ("nix", "nix profile upgrade --all || nix profile upgrade '.*'".into()),
        ("yay", "yay -Syu --noconfirm --sudo pkexec".into()),
        ("paru", "paru -Syu --noconfirm --sudo pkexec".into()),
    ]);
    let clean = by(vec![
        ("dnf", "dnf autoremove -y && dnf clean all".into()),
        ("apt", "apt-get autoremove -y && apt-get clean".into()),
        ("pacman", "pacman -Sc --noconfirm".into()),
        ("zypper", "zypper clean --all".into()),
        ("apk", "apk cache clean".into()),
        ("xbps", "xbps-remove -Oo".into()),
        ("emerge", "emerge --depclean --ask=n".into()),
        ("brew", "brew autoremove && brew cleanup".into()),
        ("nix", "nix-collect-garbage -d".into()),
        ("yay", "yay -Sc --noconfirm --sudo pkexec".into()),
        ("paru", "paru -Sc --noconfirm --sudo pkexec".into()),
    ]);
    let headers = by(vec![
        ("dnf", ic("dnf", "kernel-devel kernel-headers")),
        ("apt", ic("apt", "linux-headers-$(uname -r)")),
        ("pacman", ic("pacman", "linux-headers")),
        ("zypper", ic("zypper", "kernel-devel")),
    ]);
    let codecs = by(vec![
        (
            "dnf",
            format!("{RPMFUSION}\ndnf install -y ffmpeg --allowerasing\ndnf install -y gstreamer1-plugin-libav gstreamer1-plugins-bad-free-extras gstreamer1-plugins-ugly"),
        ),
        ("apt", ic("apt", "ffmpeg gstreamer1.0-libav gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly")),
        ("pacman", ic("pacman", "ffmpeg gst-libav gst-plugins-good gst-plugins-bad gst-plugins-ugly")),
    ]);
    let devtools = by(vec![
        ("dnf", "dnf group install -y development-tools".into()),
        ("apt", ic("apt", "build-essential")),
        ("pacman", ic("pacman", "base-devel")),
        ("zypper", "zypper --non-interactive install -t pattern devel_basis".into()),
    ]);
    let virt = by(vec![
        ("dnf", format!("{}; systemctl enable --now libvirtd", ic("dnf", "qemu-kvm libvirt virt-install virt-manager"))),
        ("apt", format!("{}; systemctl enable --now libvirtd", ic("apt", "qemu-kvm libvirt-daemon-system virtinst virt-manager"))),
        ("pacman", format!("{}; systemctl enable --now libvirtd", ic("pacman", "qemu libvirt virt-manager"))),
        ("zypper", format!("{}; systemctl enable --now libvirtd", ic("zypper", "qemu-kvm libvirt virt-install virt-manager"))),
    ]);

    Category {
        id: "system", name: "System", icon: "⚙️",
        tasks: vec![
            task("update", "Update system", "Update all installed packages", "Maintenance", update).reboot(),
            task("clean", "Clean package cache", "Remove unused packages and caches", "Maintenance", clean),
            task("journal", "Clean system logs", "Keep only the last 7 days of journal logs", "Maintenance", fixed("journalctl --vacuum-time=7d")),
            task("firmware", "Update firmware (fwupd)", "Check and install firmware updates", "Maintenance",
                fixed("fwupdmgr refresh --force; fwupdmgr get-updates; fwupdmgr update -y; rc=$?; [ $rc -eq 2 ] && { echo 'Nothing to update.'; rc=0; }; exit $rc"))
                .user().reboot(),
            task("headers", "Kernel headers / devel", "Needed to build GPU kernel modules (DKMS/akmods)", "Setup", headers).sys(),
            task("rpmfusion", "Enable RPM Fusion (Fedora)", "Free + Nonfree repositories", "Setup",
                func(|c| c.fedora.then(|| RPMFUSION.to_string()))),
            task("flathub", "Enable Flathub", "Install Flatpak and add the Flathub remote", "Setup",
                func(|c| Some(format!(
                    "{} && flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo",
                    install_cmd(&c.pm, "flatpak")?
                )))).sys(),
            task("codecs", "Multimedia codecs", "ffmpeg and GStreamer plugins", "Setup", codecs),
            task("devtools", "Build tools", "gcc, make and friends", "Setup", devtools).sys(),
            task("trim", "Enable SSD TRIM", "Weekly fstrim timer", "Tweaks", fixed("systemctl enable --now fstrim.timer")),
            task("swappiness", "Lower swappiness (10)", "Use RAM more, swap less (good for desktops)", "Tweaks",
                fixed("echo 'vm.swappiness=10' > /etc/sysctl.d/99-swappiness.conf && sysctl --system | tail -n 3")),
            task("gamemode", "GameMode", "On-demand game performance tweaks", "Tweaks", inst("gamemode", &[])),
            task("reboot", "Reboot now", "Restart the computer", "Power", fixed("systemctl reboot")).danger(),
            task("virt", "Enable virtualization", "Install QEMU/KVM and virt-manager", "Virtualization", virt),
        ],
    }
}

fn gpu() -> Category {
    let nv_remove = by(vec![
        ("dnf", "dnf remove -y '*nvidia*' --exclude=nvidia-gpu-firmware; dracut --force".into()),
        ("apt", "apt-get remove --purge -y '^nvidia-.*' && apt-get autoremove -y".into()),
        ("pacman", "pacman -Rns --noconfirm $(pacman -Qq | grep -E '^nvidia')".into()),
    ]);
    Category {
        id: "gpu", name: "GPU Drivers", icon: "🎮",
        tasks: vec![
            task("gpu-status", "GPU / driver status", "lspci, nvidia-smi and Vulkan summary", "Info",
                fixed("lspci -nnk | grep -EA3 'VGA|3D|Display'; echo; nvidia-smi 2>&1 | head -n 20; echo; vulkaninfo --summary 2>/dev/null | head -n 20")).user(),
            task("sb-state", "Secure Boot status", "NVIDIA modules need an enrolled key when Secure Boot is on", "Info",
                fixed("mokutil --sb-state 2>&1 || echo 'mokutil is not installed'")).user(),
            task("nv-auto", "Install recommended NVIDIA driver", "Detects your GPU generation and picks the right driver branch (Pascal → 580xx legacy)", "NVIDIA", func(nv_auto)).reboot(),
            task("nv-580", "NVIDIA 580xx (Maxwell / Pascal / Volta)", "GTX 750–10xx, GTX 9xx, Titan X/Xp, Quadro P/M, Tesla V100. Last branch supporting these cards", "NVIDIA",
                func(|c| if c.pm == "pacman" { arch_legacy(c, "580xx") } else { nvidia_cmd(c, "580xx") })).reboot(),
            task("nv-current", "NVIDIA current (Turing and newer)", "GTX 16xx, RTX 20/30/40/50 series", "NVIDIA", nvidia("current")).reboot(),
            task("nv-470", "NVIDIA 470xx (Kepler)", "GTX 600/700 series (older, X11 only)", "NVIDIA",
                func(|c| if c.pm == "pacman" { arch_legacy(c, "470xx") } else { nvidia_cmd(c, "470xx") })).reboot(),
            task("nv-390", "NVIDIA 390xx (Fermi)", "GTX 400/500 series (Fedora only, X11 only)", "NVIDIA", nvidia("390xx")).reboot(),
            task("nv-modeset", "Enable nvidia-drm modeset", "Recommended for Wayland and smoother login", "NVIDIA tweaks",
                func(|c| Some(format!(
                    "echo 'options nvidia-drm modeset=1' > /etc/modprobe.d/nvidia-drm-modeset.conf && {}",
                    rebuild_initramfs(&c.pm)
                )))).reboot(),
            task("nv-rebuild", "Rebuild NVIDIA module (Fedora)", "Force akmods to rebuild after a kernel update", "NVIDIA tweaks",
                func(|c| c.fedora.then(|| "akmods --force && dracut --force".to_string()))).reboot(),
            task("nv-sb", "Secure Boot: key enrollment help", "Shows the steps to enroll the akmods key (needs a terminal password prompt)", "NVIDIA tweaks",
                fixed("echo 'Run in a terminal:'; echo '  sudo kmodgenca -a'; echo '  sudo mokutil --import /etc/pki/akmods/certs/public_key.der'; echo 'Set a one-time password, reboot, choose Enroll MOK, enter the password.'; echo 'Alternatively disable Secure Boot in firmware setup.'")).user(),
            task("nv-remove", "Remove NVIDIA drivers", "Uninstall proprietary NVIDIA packages (falls back to nouveau)", "NVIDIA tweaks", nv_remove).danger().reboot(),
            task("amd", "AMD graphics stack", "Mesa, Vulkan (RADV), VA-API tools", "AMD / Intel",
                inst("mesa-vulkan-drivers vulkan-tools libva-utils", &[
                    ("apt", Some("mesa-vulkan-drivers mesa-va-drivers vulkan-tools vainfo")),
                    ("pacman", Some("mesa vulkan-radeon libva-mesa-driver vulkan-tools libva-utils")),
                    ("zypper", Some("Mesa-dri vulkan-tools libva-utils")),
                ])),
            task("intel", "Intel graphics stack", "Media driver, Vulkan, VA-API tools", "AMD / Intel",
                inst("intel-media-driver vulkan-tools libva-utils", &[
                    ("apt", Some("intel-media-va-driver mesa-vulkan-drivers vulkan-tools vainfo")),
                    ("pacman", Some("intel-media-driver vulkan-intel vulkan-tools libva-utils")),
                    ("zypper", Some("intel-media-driver vulkan-tools libva-utils")),
                ])),
            task("vulkan-tools", "Vulkan + OpenGL tools", "vulkaninfo, glxinfo, vkcube", "Tools",
                inst("vulkan-tools mesa-demos", &[
                    ("apt", Some("vulkan-tools mesa-utils")),
                    ("pacman", Some("vulkan-tools mesa-utils")),
                    ("zypper", Some("vulkan-tools Mesa-demo-x")),
                ])),
        ],
    }
}

fn apps() -> Category {
    let brave_script = "curl -fsS https://dl.brave.com/install.sh | sh";
    let brave_cmd = by(vec![
        ("dnf", brave_script.into()),
        ("apt", brave_script.into()),
        ("zypper", brave_script.into()),
    ]);

    let docker_post = "systemctl enable --now docker && usermod -aG docker $(id -nu $PKEXEC_UID)";
    let docker = by(vec![
        ("dnf", format!("{} && {docker_post}", ic("dnf", "moby-engine docker-compose"))),
        ("apt", format!("{} && {docker_post}", ic("apt", "docker.io docker-compose"))),
        ("pacman", format!("{} && {docker_post}", ic("pacman", "docker docker-compose"))),
    ]);

    let vscode = by(vec![
        (
            "dnf",
            lines(&[
                "rpm --import https://packages.microsoft.com/keys/microsoft.asc",
                "cat > /etc/yum.repos.d/vscode.repo <<'EOF'",
                "[code]",
                "name=Visual Studio Code",
                "baseurl=https://packages.microsoft.com/yumrepos/vscode",
                "enabled=1",
                "autorefresh=1",
                "type=rpm-md",
                "gpgcheck=1",
                "gpgkey=https://packages.microsoft.com/keys/microsoft.asc",
                "EOF",
                "dnf install -y code",
            ]),
        ),
        (
            "apt",
            apt_repo(
                "https://packages.microsoft.com/keys/microsoft.asc",
                "microsoft.gpg",
                "deb [arch=amd64,arm64,armhf signed-by=/usr/share/keyrings/microsoft.gpg] https://packages.microsoft.com/repos/code stable main",
                "vscode.list",
                "code",
            ),
        ),
    ]);

    let digidoc_cmd = by(vec![
        ("dnf", digidoc("dnf", "qdigidoc pcsc-lite pcsc-lite-ccid opensc", "web-eid", "web-eid not in repos: install the Web eID browser extension instead")),
        ("apt", digidoc("apt", "qdigidoc4 pcscd pcsc-tools opensc", "web-eid", "web-eid not in repos: install the Web eID browser extension instead")),
        ("pacman", digidoc("pacman", "pcsclite ccid opensc", "qdigidoc4", "qdigidoc4 not in official repos: install it from the AUR")),
        ("zypper", digidoc("zypper", "pcsc-lite pcsc-ccid opensc", "qdigidoc4", "qdigidoc4 not in default repos: add the RIA/OBS repository")),
    ]);

    let signal = by(vec![
        ("dnf", ic("dnf", "signal-desktop")),
        (
            "apt",
            apt_repo(
                "https://updates.signal.org/desktop/apt/keys.asc",
                "signal-desktop-keyring.gpg",
                "deb [arch=amd64 signed-by=/usr/share/keyrings/signal-desktop-keyring.gpg] https://updates.signal.org/desktop/apt xenial main",
                "signal-xenial.list",
                "signal-desktop",
            ),
        ),
        ("pacman", ic("pacman", "signal-desktop")),
        ("zypper", ic("zypper", "signal-desktop")),
    ]);
    let slack = by(vec![
        ("dnf", ic("dnf", "slack")),
        (
            "apt",
            apt_repo(
                "https://packagecloud.io/slacktechnologies/slack/gpgkey",
                "slack-keyring.gpg",
                "deb [arch=amd64 signed-by=/usr/share/keyrings/slack-keyring.gpg] https://packagecloud.io/slacktechnologies/slack/debian/ jessie main",
                "slack.list",
                "slack-desktop",
            ),
        ),
        ("pacman", ic("pacman", "slack-desktop")),
        ("zypper", ic("zypper", "slack")),
    ]);
    let discord = by(vec![
        ("dnf", ic("dnf", "discord.x86_64")),
        (
            "apt",
            apt_repo(
                "https://discordapp.com/api/download?platform=linux&format=deb",
                "discord-keyring.gpg",
                "deb [arch=amd64 signed-by=/usr/share/keyrings/discord-keyring.gpg] https://discordapp.com/api/download?platform=linux&format=deb stable main",
                "discord.list",
                "discord",
            ),
        ),
        ("pacman", ic("pacman", "discord")),
        ("zypper", ic("zypper", "discord")),
    ]);

    Category {
        id: "apps", name: "Applications", icon: "📦",
        tasks: vec![
            task("firefox", "Firefox", "Web browser", "Browsers", inst("firefox", &[])),
            task("chromium", "Chromium", "Web browser", "Browsers", inst("chromium", &[])),
            task("brave", "Brave", "Privacy browser (official Brave repo script; Fedora, Ubuntu/Debian, openSUSE)", "Browsers", brave_cmd),
            task("zen", "Zen Browser (Flatpak)", "Firefox-based browser focused on privacy and customization", "Browsers", fixed(&flatpak_cmd("app.zen_browser.zen"))).user(),
            task("git", "Git", "Version control", "Development", inst("git", &[])),
            task("node", "Node.js + npm", "JavaScript runtime", "Development", inst("nodejs npm", &[])),
            task("python", "Python 3 + pip", "Python runtime", "Development",
                inst("python3 python3-pip", &[("pacman", Some("python python-pip"))])),
            task("docker", "Docker", "Container engine (adds you to the docker group)", "Development", docker),
            task("vscode", "Visual Studio Code", "Code editor (official Microsoft repo)", "Development", vscode),
            task("vscode-flatpak", "Visual Studio Code (Flatpak)", "Works on every distro, including Arch", "Development", fixed(&flatpak_cmd("com.visualstudio.code"))).user(),
            task("zed", "Zed", "Fast code editor (official install script, installs to ~/.local)", "Development", fixed("curl -f https://zed.dev/install.sh | sh")).user(),
            task("vlc", "VLC", "Media player", "Media", inst("vlc", &[])),
            task("mpv", "mpv", "Lightweight media player", "Media", inst("mpv", &[])),
            task("obs", "OBS Studio", "Recording and streaming (Fedora: needs RPM Fusion)", "Media", inst("obs-studio", &[])),
            task("steam", "Steam", "Fedora: needs RPM Fusion. Arch: needs multilib. Ubuntu: multiverse", "Gaming",
                inst("steam", &[("apt", Some("steam-installer"))])),
            task("lutris", "Lutris", "Game launcher", "Gaming", inst("lutris", &[])),
            task("mangohud", "MangoHud", "FPS / performance overlay", "Gaming", inst("mangohud", &[])),
            task("wine", "Wine", "Run Windows apps", "Gaming", inst("wine", &[])),
            task("discord", "Discord (Flatpak)", "Chat", "Flatpak apps", fixed(&flatpak_cmd("com.discordapp.Discord"))).user(),
            task("spotify", "Spotify (Flatpak)", "Music", "Flatpak apps", fixed(&flatpak_cmd("com.spotify.Client"))).user(),
            task("heroic", "Heroic Launcher (Flatpak)", "Epic / GOG games", "Flatpak apps", fixed(&flatpak_cmd("com.heroicgameslauncher.hgl"))).user(),
            task("htop", "htop", "Process viewer", "Terminal tools", inst("htop", &[])),
            task("btop", "btop", "Resource monitor", "Terminal tools", inst("btop", &[])),
            task("fastfetch", "fastfetch", "System info in the terminal", "Terminal tools", inst("fastfetch", &[])),
            task("neovim", "Neovim", "Text editor", "Terminal tools", inst("neovim", &[])),
            task("tmux", "tmux", "Terminal multiplexer", "Terminal tools", inst("tmux", &[])),
            task("filezilla", "FileZilla", "FTP client (Homebrew: via Flatpak)", "Tools",
                func(|c| {
                    if c.pm == "brew" {
                        Some(flatpak_cmd("org.filezillaproject.Filezilla"))
                    } else if c.pm == "emerge" {
                        install_cmd(&c.pm, "net-ftp/filezilla")
                    } else {
                        install_cmd(&c.pm, "filezilla")
                    }
                })),
            task("nginx", "NGINX web server", "Install and start NGINX", "Web tools",
                func(|c| {
                    let i = install_cmd(&c.pm, nginx_pkg(&c.pm))?;
                    Some(join(&[&i, &svc("nginx")]))
                })).sys(),
            task("php", "PHP complete stack", "Install PHP-FPM, MariaDB and NGINX", "Web tools",
                func(|c| {
                    let p = php_pkgs(&c.pm)?;
                    let d = db(&c.pm)?;
                    let prefix = if c.pm == "emerge" { r#"USE="fpm mysql mysqli pdo" "# } else { "" };
                    let install = install_cmd(&c.pm, &format!("{p} {} {}", nginx_pkg(&c.pm), d.0))?;
                    Some(join(&[
                        &format!("{prefix}{install}"),
                        d.1,
                        php_setup(&c.pm),
                        &php_svc(&c.pm),
                        &svc("nginx"),
                        &svc(d.2),
                    ]))
                })).sys(),
            task("mysql", "MySQL / MariaDB", "Install and start the MariaDB server (MySQL compatible)", "Web tools",
                func(|c| {
                    let d = db(&c.pm)?;
                    Some(join(&[&install_cmd(&c.pm, d.0)?, d.1, &svc(d.2)]))
                })).sys(),
            task("mongodb", "MongoDB", "Official MongoDB Community repo: Fedora/RHEL, Ubuntu/Debian, openSUSE. Arch via AUR", "Web tools",
                func(|c| Some(match c.pm.as_str() {
                    "dnf" => format!("{}\n{}", mongo_dnf(), svc("mongod")),
                    "apt" => format!("{}\n{}", mongo_apt(), svc("mongod")),
                    "zypper" => format!("{}\n{}", mongo_zypper(), svc("mongod")),
                    "yay" => join(&[&ic("yay", "mongodb-bin"), &svc("mongodb")]),
                    "paru" => join(&[&ic("paru", "mongodb-bin"), &svc("mongodb")]),
                    _ => return None,
                }))).sys(),
            task("mongodb-compass", "MongoDB Compass",
                &format!("Official MongoDB GUI {COMPASS_VER} (from mongodb.com; x86_64 only). Arch via AUR"), "Web tools",
                func(compass)).sys(),
            task("digidoc", "DigiDoc4 / Web EID", "Estonian ID card / Smart ID / Mobile ID", "Tools", digidoc_cmd),
            task("thunderbird", "Thunderbird", "Email client", "Communications", inst("thunderbird", &[])),
            task("signal", "Signal", "Secure messaging app", "Communications", signal),
            task("slack", "Slack", "Team chat app", "Communications", slack),
            task("discord-native", "Discord", "Chat app for communities", "Communications", discord),
            task("rust", "Rust", "Install Rust via rustup (official method)", "Development",
                fixed("curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y")).user(),
            /* Install DBeaver  https://dbeaver.io/files/dbeaver-ce-latest-linux-x86_64.rpm */
            
            task(
                "dbeaver-ce",
                "DBeaver",
                "Database management tool (Official RPM/DEB, Arch, Nix, Flatpak)",
                "Development",
                func(|c| {
                    let flatpak = |pkg: &str| -> Option<String> {
                        Some(format!(
                            "{} && flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install -y flathub io.dbeaver.DBeaverCommunity",
                            install_cmd(&c.pm, pkg)?
                        ))
                    };

                    match c.pm.as_str() {
                        // Fedora / RHEL / CentOS
                        "dnf" => Some(lines(&[
                            "set -eu",
                            r#"
            ARCH="$(uname -m)"

            case "$ARCH" in
                x86_64|aarch64) ;;
                *)
                    echo "Unsupported architecture: $ARCH"
                    exit 1
                    ;;
            esac

            TMP="$(mktemp --suffix=.rpm)"
            trap 'rm -f "$TMP"' EXIT

            dnf install -y curl ca-certificates

            curl -fSL --retry 3 \
                "https://dbeaver.io/files/dbeaver-ce-latest-linux-${ARCH}.rpm" \
                -o "$TMP"

            dnf install -y "$TMP"
            "#,
                        ])),

                        // openSUSE
                        "zypper" => Some(lines(&[
                            "set -eu",
                            r#"
            ARCH="$(uname -m)"

            case "$ARCH" in
                x86_64|aarch64) ;;
                *)
                    echo "Unsupported architecture: $ARCH"
                    exit 1
                    ;;
            esac

            TMP="$(mktemp --suffix=.rpm)"
            trap 'rm -f "$TMP"' EXIT

            zypper --non-interactive install curl ca-certificates

            curl -fSL --retry 3 \
                "https://dbeaver.io/files/dbeaver-ce-latest-linux-${ARCH}.rpm" \
                -o "$TMP"

            zypper --non-interactive --no-gpg-checks install "$TMP"
            "#,
                        ])),

                        // Debian / Ubuntu / Linux Mint / Pop!_OS
                        "apt" => Some(lines(&[
                            "set -eu",
                            "export DEBIAN_FRONTEND=noninteractive",
                            r#"
            ARCH="$(dpkg --print-architecture)"

            case "$ARCH" in
                amd64) ARCH="x86_64" ;;
                arm64) ARCH="aarch64" ;;
                *)
                    echo "Unsupported architecture: $ARCH"
                    exit 1
                    ;;
            esac

            apt-get update
            apt-get install -y curl ca-certificates

            TMP="$(mktemp --suffix=.deb)"
            trap 'rm -f "$TMP"' EXIT

            curl -fSL --retry 3 \
                "https://dbeaver.io/files/dbeaver-ce-latest-linux-${ARCH}.deb" \
                -o "$TMP"

            apt-get install -y "$TMP"
            "#,
                        ])),

                        // Arch Linux / AUR
                        "pacman" | "yay" | "paru" => {
                            install_cmd(&c.pm, "dbeaver")
                        }

                        // NixOS
                        "nix" => {
                            install_cmd("nix", "dbeaver-bin")
                        }

                        // Alpine / Void Linux
                        "apk" | "xbps" => {
                            flatpak("flatpak")
                        }

                        // Gentoo
                        "emerge" => {
                            flatpak("sys-apps/flatpak")
                        }

                        // Unsupported package manager
                        _ => None,
                    }
                }),
            ),



            task(
                "heidisql",
                "HeidiSQL",
                "Database management tool (Official RPM/DEB, Arch, Flatpak)",
                "Development",
                func(|c| {
                    let version = "12.22";

                    let rpm = format!(
                        "https://github.com/HeidiSQL/HeidiSQL/releases/download/v{version}/heidisql-{version}-1.x86_64.rpm"
                    );

                    let deb = format!(
                        "https://github.com/HeidiSQL/HeidiSQL/releases/download/v{version}/heidisql_{version}_amd64.deb"
                    );

                    let flatpak = |pkg: &str| -> Option<String> {
                        Some(format!(
                            "{} && \
                            flatpak remote-add --if-not-exists flatpark \
                            https://dl.flatpark.org/flatpark.flatpakrepo && \
                            flatpak install -y flatpark com.heidisql.HeidiSQL",
                            install_cmd(&c.pm, pkg)?
                        ))
                    };

                    match c.pm.as_str() {
                        // Fedora / RHEL / CentOS
                        "dnf" => Some(lines(&[
                            "set -eu",
                            r#"
                            ARCH="$(uname -m)"

                            if [ "$ARCH" != "x86_64" ]; then
                                echo "Official RPM package currently supports x86_64 only."
                                exit 1
                            fi
                            "#,
                            "dnf install -y curl ca-certificates",
                            "TMP=\"$(mktemp --suffix=.rpm)\"",
                            "trap 'rm -f \"$TMP\"' EXIT",
                            &format!(
                                "curl -fSL --retry 3 -o \"$TMP\" \"{rpm}\""
                            ),
                            "dnf install -y \"$TMP\"",
                        ])),

                        // openSUSE
                        "zypper" => Some(lines(&[
                            "set -eu",
                            r#"
                            if [ "$(uname -m)" != "x86_64" ]; then
                                echo "Official RPM package currently supports x86_64 only."
                                exit 1
                            fi
                            "#,
                            "zypper --non-interactive install curl ca-certificates",
                            "TMP=\"$(mktemp --suffix=.rpm)\"",
                            "trap 'rm -f \"$TMP\"' EXIT",
                            &format!(
                                "curl -fSL --retry 3 -o \"$TMP\" \"{rpm}\""
                            ),
                            "zypper --non-interactive install \"$TMP\"",
                        ])),

                        // Debian / Ubuntu / Linux Mint / Pop!_OS
                        "apt" => Some(lines(&[
                            "set -eu",
                            "export DEBIAN_FRONTEND=noninteractive",
                            r#"
                            ARCH="$(dpkg --print-architecture)"

                            if [ "$ARCH" != "amd64" ]; then
                                echo "Official DEB package currently supports amd64 only."
                                exit 1
                            fi
                            "#,
                            "apt-get update",
                            "apt-get install -y curl ca-certificates",
                            "TMP=\"$(mktemp --suffix=.deb)\"",
                            "trap 'rm -f \"$TMP\"' EXIT",
                            &format!(
                                "curl -fSL --retry 3 -o \"$TMP\" \"{deb}\""
                            ),
                            "apt-get install -y \"$TMP\"",
                        ])),

                        // Arch Linux / AUR
                        "pacman" | "yay" | "paru" => {
                            install_cmd(&c.pm, "heidisql")
                        }

                        // NixOS
                        "nix" => {
                            install_cmd("nix", "heidisql")
                        }

                        // Alpine / Void Linux
                        "apk" | "xbps" => {
                            flatpak("flatpak")
                        }

                        // Gentoo
                        "emerge" => {
                            flatpak("sys-apps/flatpak")
                        }

                        // Unsupported package manager
                        _ => None,
                    }
                }),
            ),


            
                
        ],
    }
}

fn security() -> Category {
    let autoupdate = by(vec![
        ("dnf", format!("{} && systemctl enable --now dnf-automatic-install.timer", ic("dnf", "dnf-automatic"))),
        ("apt", ic("apt", "unattended-upgrades")),
    ]);
    Category {
        id: "security", name: "Security", icon: "🛡️",
        tasks: vec![
            task("firewall", "Enable firewall", "firewalld on Fedora/openSUSE, ufw elsewhere", "Protection",
                func(|c| if c.pm == "dnf" || c.pm == "zypper" {
                    Some(format!("{} && systemctl enable --now firewalld && firewall-cmd --state", install_cmd(&c.pm, "firewalld")?))
                } else {
                    Some(format!(
                        "{} && ufw default deny incoming && ufw default allow outgoing && ufw --force enable && ufw status",
                        install_cmd(&c.pm, "ufw")?
                    ))
                })).sys(),
            task("fail2ban", "Fail2ban", "Ban IPs with repeated failed logins", "Protection",
                func(|c| Some(format!("{} && systemctl enable --now fail2ban", install_cmd(&c.pm, "fail2ban")?)))).sys(),
            task("autoupdate", "Automatic security updates", "dnf-automatic / unattended-upgrades", "Protection", autoupdate),
            task("clamav", "ClamAV", "Antivirus scanner", "Protection", inst("clamav", &[("dnf", Some("clamav clamav-update"))])),
            task("selinux", "SELinux status", "Show SELinux mode (Fedora)", "Info",
                fixed("getenforce 2>&1 || sestatus 2>&1 || echo 'SELinux not available'")).user(),
            task("fw-status", "Firewall status", "Show firewall state", "Info",
                fixed("(firewall-cmd --state 2>&1) || (ufw status 2>&1) || echo 'No firewall found'")).user(),
            task("listening", "Listening ports", "Open network ports and their programs", "Info", fixed("ss -tulpn 2>&1 | head -n 40")).user(),
        ],
    }
}

fn utils() -> Category {
    Category {
        id: "utils", name: "Utilities", icon: "🧰",
        tasks: vec![
            task("sysinfo", "System info", "Kernel, CPU, memory, disks", "Info",
                fixed("uname -a; echo; lscpu | head -n 16; echo; free -h; echo; df -h -x tmpfs -x devtmpfs")).user(),
            task("disk-usage", "Biggest folders in home", "Top 15 largest folders", "Info",
                fixed(r#"du -h --max-depth=1 "$HOME" 2>/dev/null | sort -hr | head -n 15"#)).user(),
            task("top-mem", "Top memory processes", "10 biggest RAM users", "Info", fixed("ps aux --sort=-%mem | head -n 11")).user(),
            task("failed", "Failed services", "systemd units in failed state", "Info", fixed("systemctl --failed --no-pager")).user(),
            task("boot-time", "Boot time analysis", "Slowest startup services", "Info", fixed("systemd-analyze; echo; systemd-analyze blame | head -n 15")).user(),
            task("smart", "Disk health (SMART)", "Install smartmontools and scan drives", "Hardware",
                func(|c| Some(format!(
                    "{} && smartctl --scan && for d in $(smartctl --scan | awk '{{print $1}}'); do smartctl -H $d; done",
                    install_cmd(&c.pm, "smartmontools")?
                )))).sys(),
            task("net-info", "Network info", "IP addresses and DNS", "Network", fixed("ip -br a; echo; resolvectl status 2>/dev/null | head -n 20")).user(),
            task("restart-net", "Restart network", "Restart NetworkManager", "Network", fixed("systemctl restart NetworkManager")),
            task("flatpak-update", "Update Flatpaks", "Update all Flatpak apps", "Maintenance", fixed("flatpak update -y")).user(),
            task("clear-cache", "Clear user cache", "Delete ~/.cache contents", "Maintenance", fixed(r#"rm -rf "$HOME"/.cache/* && echo Done"#)).user().danger(),
        ],
    }
}
