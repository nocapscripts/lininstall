// Task fields: id, name, sys? (system-level, hidden for brew/nix), desc, group?, root (default true), reboot?, danger?
// cmd is a string, or a function (ctx) => string | undefined (undefined = not supported on this system)
const installCmd = (pm, l) => ({
  apt: `export DEBIAN_FRONTEND=noninteractive; apt-get install -y ${l}`,
  pacman: `pacman -S --noconfirm --needed ${l}`,
  dnf: `dnf install -y ${l}`,
  zypper: `zypper --non-interactive install ${l}`,
  apk: `apk add ${l}`,
  xbps: `xbps-install -Sy ${l}`,
  emerge: `emerge --ask=n --noreplace ${l}`,
  brew: `brew install ${l}`,
  nix: `nix profile install ${String(l).split(" ").map((p) => "nixpkgs#" + p).join(" ")}`,
  yay: `yay -S --noconfirm --needed --sudo pkexec ${l}`,
  paru: `paru -S --noconfirm --needed --sudo pkexec ${l}`,
})[pm];

// inst("pkg", { pacman: "other-name", zypper: null })  -> null/undefined means unsupported
const inst = (all, over = {}) => (ctx) => {
  const l = ctx.pm in over ? over[ctx.pm] : all;
  return l ? installCmd(ctx.pm, l) : undefined;
};
const by = (map) => (ctx) => map[ctx.pm];
const flatpak = (id) => ({
  root: false,
  cmd: `flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install --user -y flathub ${id}`,
});

const RPMFUSION = `dnf install -y https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm`;
const REBUILD_INITRAMFS = { dnf: "dracut --force", zypper: "dracut --force", apt: "update-initramfs -u", pacman: "mkinitcpio -P" };

// ---------- NVIDIA ----------
// branch: "current" (Turing+), "580xx" (Maxwell/Pascal/Volta), "470xx" (Kepler), "390xx" (Fermi)
const nvidia = (branch) => (ctx) => {
  if (ctx.pm === "dnf" && ctx.fedora) {
    const s = branch === "current" ? "" : "-" + branch;
    return [
      "set -e",
      RPMFUSION,
      "dnf remove -y 'xorg-x11-drv-nvidia*' 'akmod-nvidia*' || true",
      `dnf install -y kernel-devel akmod-nvidia${s} xorg-x11-drv-nvidia${s} xorg-x11-drv-nvidia${s}-cuda`,
      "echo 'Building kernel module (this can take several minutes)...'",
      "akmods --force",
      "dracut --force",
      "echo; echo 'Done. Reboot to load the driver.'",
    ].join("\n");
  }
  if (ctx.pm === "apt") {
    if (branch === "390xx") return undefined;
    const legacy = { "580xx": "nvidia-driver-580", "470xx": "nvidia-driver-470" }[branch];
    return [
      "export DEBIAN_FRONTEND=noninteractive",
      "apt-get update",
      "if command -v ubuntu-drivers >/dev/null 2>&1; then",
      legacy ? `  apt-get install -y ${legacy} || ubuntu-drivers install` : "  ubuntu-drivers install",
      "else",
      "  apt-get install -y nvidia-driver firmware-misc-nonfree",
      "fi",
      "echo; echo 'Done. Reboot to load the driver.'",
    ].join("\n");
  }
  if (ctx.pm === "pacman") {
    if (branch === "current")
      return "pacman -S --noconfirm --needed linux-headers nvidia-open-dkms nvidia-utils nvidia-settings && echo 'Done. Reboot.'";
    // Legacy drivers live in the AUR - needs an AUR helper run as a normal user
    if (branch === "580xx" || branch === "470xx") return undefined;
  }
  return undefined;
};
// Arch legacy (AUR) - runs as the normal user via yay/paru
const archLegacy = (branch) => (ctx) => {
  if (ctx.pm !== "pacman") return undefined;
  const pkgs = branch === "580xx" ? "nvidia-580xx-dkms nvidia-580xx-utils" : "nvidia-470xx-dkms nvidia-470xx-utils";
  return `H=$(command -v yay || command -v paru) || { echo 'Install yay or paru first (AUR helper needed).'; exit 1; }; sudo pacman -S --needed --noconfirm linux-headers; $H -S --needed ${pkgs}`;
};

const nvAuto = (ctx) => {
  const g = ctx.nvidia;
  if (!g) return "echo 'No NVIDIA GPU detected (is pciutils/lspci installed?).'";
  if (!g.branch) return "echo 'Could not determine the NVIDIA generation. Pick a driver manually.'";
  if (ctx.pm === "pacman" && g.branch !== "current") return archLegacy(g.branch)(ctx);
  return nvidia(g.branch)(ctx);
};

// ---------- Web tools helpers ----------
// Enable + start a service on systemd, OpenRC (Alpine/Gentoo) or runit (Void)
const svc = (n) => `if command -v systemctl >/dev/null 2>&1; then systemctl enable --now ${n}; elif command -v rc-update >/dev/null 2>&1; then rc-update add ${n} default && rc-service ${n} start; elif [ -d /etc/sv/${n} ]; then ln -sf /etc/sv/${n} /var/service/${n}; fi`;
const initDb = "[ -d /var/lib/mysql/mysql ] || mariadb-install-db --user=mysql --basedir=/usr --datadir=/var/lib/mysql";
// [packages, setup before start, service name]
const DB = {
  dnf: ["mariadb-server", "", "mariadb"],
  apt: ["mariadb-server", "", "mariadb"],
  zypper: ["mariadb", "", "mariadb"],
  pacman: ["mariadb", initDb, "mariadb"],
  yay: ["mariadb", initDb, "mariadb"],
  paru: ["mariadb", initDb, "mariadb"],
  apk: ["mariadb mariadb-client", initDb, "mariadb"],
  xbps: ["mariadb", initDb, "mariadb"],
  emerge: ["dev-db/mariadb", initDb, "mysql"],
};
const PHP_PKGS = {
  dnf: "php php-mysqlnd php-fpm", apt: "php php-mysql php-fpm", zypper: "php8 php8-mysql php8-fpm",
  pacman: "php php-fpm", yay: "php php-fpm", paru: "php php-fpm",
  apk: "php83 php83-fpm php83-mysqlnd php83-pdo_mysql php83-mysqli", xbps: "php php-fpm php-mysql", emerge: "dev-lang/php",
};
const PHP_SETUP = {
  zypper: "cp -n /etc/php8/fpm/php-fpm.conf.default /etc/php8/fpm/php-fpm.conf; cp -n /etc/php8/fpm/php-fpm.d/www.conf.default /etc/php8/fpm/php-fpm.d/www.conf",
  pacman: "sed -i 's/^;extension=mysqli/extension=mysqli/; s/^;extension=pdo_mysql/extension=pdo_mysql/' /etc/php/php.ini",
};
PHP_SETUP.yay = PHP_SETUP.paru = PHP_SETUP.pacman;
const phpSvc = (pm) => pm === "apt"
  ? "systemctl enable --now $(systemctl list-unit-files --no-legend 'php*-fpm.service' | awk 'NR==1{print $1}')"
  : svc(pm === "apk" ? "php-fpm83" : "php-fpm");
const NGINX = (pm) => (pm === "emerge" ? "www-servers/nginx" : "nginx");
const join = (...p) => p.filter(Boolean).join(" && ");

// MongoDB Community series - see https://www.mongodb.com/try/download/community
const MONGO_VER = "9.0";
const MONGO_DNF = [
  "set -e",
  "V=$(rpm -E %rhel); case \"$V\" in 8|9|10) ;; *) V=9 ;; esac   # Fedora uses the RHEL 9 build",
  `rpm --import https://pgp.mongodb.com/server-${MONGO_VER}.asc`,
  `cat > /etc/yum.repos.d/mongodb-org-${MONGO_VER}.repo <<EOF`,
  `[mongodb-org-${MONGO_VER}]`,
  "name=MongoDB Repository",
  `baseurl=https://repo.mongodb.org/yum/redhat/$V/mongodb-org/${MONGO_VER}/$(uname -m)/`,
  "gpgcheck=1",
  "enabled=1",
  `gpgkey=https://pgp.mongodb.com/server-${MONGO_VER}.asc`,
  "EOF",
  "dnf install -y mongodb-org",
].join("\n");
const MONGO_APT = [
  "set -e",
  "export DEBIAN_FRONTEND=noninteractive",
  "apt-get update && apt-get install -y gnupg curl ca-certificates",
  ". /etc/os-release",
  "CN=$UBUNTU_CODENAME; [ -n \"$CN\" ] || CN=$VERSION_CODENAME",
  "case \"$ID $ID_LIKE\" in",
  "  *ubuntu*) set -- ubuntu $CN multiverse ;;",
  "  *) set -- debian $CN main ;;",
  "esac",
  `curl -fsSL https://pgp.mongodb.com/server-${MONGO_VER}.asc | gpg --dearmor --yes -o /usr/share/keyrings/mongodb-server-${MONGO_VER}.gpg`,
  `echo "deb [ signed-by=/usr/share/keyrings/mongodb-server-${MONGO_VER}.gpg ] https://repo.mongodb.org/apt/$1 $2/mongodb-org/${MONGO_VER} $3" > /etc/apt/sources.list.d/mongodb-org-${MONGO_VER}.list`,
  "apt-get update && apt-get install -y mongodb-org",
].join("\n");
const MONGO_ZYPPER = [
  "set -e",
  ". /etc/os-release",
  "V=$(echo \"$VERSION_ID\" | cut -d. -f1); case \"$V\" in 15|16) ;; *) V=15 ;; esac",
  `rpm --import https://pgp.mongodb.com/server-${MONGO_VER}.asc`,
  `zypper --non-interactive removerepo mongodb-org-${MONGO_VER} 2>/dev/null || true`,
  `zypper --non-interactive addrepo --gpgcheck "https://repo.mongodb.org/zypper/suse/$V/mongodb-org/${MONGO_VER}/$(uname -m)/" mongodb-org-${MONGO_VER}`,
  "zypper --non-interactive --gpg-auto-import-keys refresh",
  "zypper --non-interactive install mongodb-org",
].join("\n");

const COMPASS_VER = "1.53.2";

module.exports = [
  {
    id: "system", name: "System", icon: "⚙️",
    tasks: [
      { id: "update", name: "Update system", desc: "Update all installed packages", group: "Maintenance", reboot: true,
        cmd: by({ dnf: "dnf upgrade -y --refresh", apt: "export DEBIAN_FRONTEND=noninteractive; apt-get update && apt-get upgrade -y", pacman: "pacman -Syu --noconfirm", zypper: "zypper --non-interactive update",
          apk: "apk update && apk upgrade", xbps: "xbps-install -Syu", emerge: "emerge --sync && emerge --ask=n -uDN @world",
          brew: "brew update && brew upgrade", nix: "nix profile upgrade --all || nix profile upgrade '.*'",
          yay: "yay -Syu --noconfirm --sudo pkexec", paru: "paru -Syu --noconfirm --sudo pkexec" }) },
      { id: "clean", name: "Clean package cache", desc: "Remove unused packages and caches", group: "Maintenance",
        cmd: by({ dnf: "dnf autoremove -y && dnf clean all", apt: "apt-get autoremove -y && apt-get clean", pacman: "pacman -Sc --noconfirm", zypper: "zypper clean --all",
          apk: "apk cache clean", xbps: "xbps-remove -Oo", emerge: "emerge --depclean --ask=n",
          brew: "brew autoremove && brew cleanup", nix: "nix-collect-garbage -d",
          yay: "yay -Sc --noconfirm --sudo pkexec", paru: "paru -Sc --noconfirm --sudo pkexec" }) },
      { id: "journal", name: "Clean system logs", desc: "Keep only the last 7 days of journal logs", group: "Maintenance", cmd: "journalctl --vacuum-time=7d" },
      { id: "firmware", name: "Update firmware (fwupd)", desc: "Check and install firmware updates", group: "Maintenance", root: false, reboot: true,
        cmd: "fwupdmgr refresh --force; fwupdmgr get-updates; fwupdmgr update -y; rc=$?; [ $rc -eq 2 ] && { echo 'Nothing to update.'; rc=0; }; exit $rc" },
      { id: "headers", sys: true, name: "Kernel headers / devel", desc: "Needed to build GPU kernel modules (DKMS/akmods)", group: "Setup",
        cmd: by({ dnf: installCmd("dnf", "kernel-devel kernel-headers"), apt: installCmd("apt", "linux-headers-$(uname -r)"), pacman: installCmd("pacman", "linux-headers"), zypper: installCmd("zypper", "kernel-devel") }) },
      { id: "rpmfusion", name: "Enable RPM Fusion (Fedora)", desc: "Free + Nonfree repositories", group: "Setup",
        cmd: (ctx) => (ctx.fedora ? RPMFUSION : undefined) },
      { id: "flathub", sys: true, name: "Enable Flathub", desc: "Install Flatpak and add the Flathub remote", group: "Setup",
        cmd: (ctx) => `${installCmd(ctx.pm, "flatpak")} && flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo` },
      { id: "codecs", name: "Multimedia codecs", desc: "ffmpeg and GStreamer plugins", group: "Setup",
        cmd: by({
          dnf: `${RPMFUSION}\ndnf install -y ffmpeg --allowerasing\ndnf install -y gstreamer1-plugin-libav gstreamer1-plugins-bad-free-extras gstreamer1-plugins-ugly`,
          apt: installCmd("apt", "ffmpeg gstreamer1.0-libav gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly"),
          pacman: installCmd("pacman", "ffmpeg gst-libav gst-plugins-good gst-plugins-bad gst-plugins-ugly"),
        }) },
      { id: "devtools", sys: true, name: "Build tools", desc: "gcc, make and friends", group: "Setup",
        cmd: by({ dnf: "dnf group install -y development-tools", apt: installCmd("apt", "build-essential"), pacman: installCmd("pacman", "base-devel"), zypper: "zypper --non-interactive install -t pattern devel_basis" }) },
      { id: "trim", name: "Enable SSD TRIM", desc: "Weekly fstrim timer", group: "Tweaks", cmd: "systemctl enable --now fstrim.timer" },
      { id: "swappiness", name: "Lower swappiness (10)", desc: "Use RAM more, swap less (good for desktops)", group: "Tweaks",
        cmd: "echo 'vm.swappiness=10' > /etc/sysctl.d/99-swappiness.conf && sysctl --system | tail -n 3" },
      { id: "gamemode", name: "GameMode", desc: "On-demand game performance tweaks", group: "Tweaks",
        cmd: inst("gamemode") },
      { id: "reboot", name: "Reboot now", desc: "Restart the computer", group: "Power", danger: true, cmd: "systemctl reboot" },
      /* Enable virtualization */
      { id: "virt", name: "Enable virtualization", desc: "Install QEMU/KVM and virt-manager", group: "Virtualization",
        cmd: by({
          dnf: `${installCmd("dnf", "qemu-kvm libvirt virt-install virt-manager")}; systemctl enable --now libvirtd`,
          apt: `${installCmd("apt", "qemu-kvm libvirt-daemon-system virtinst virt-manager")}; systemctl enable --now libvirtd`,
          pacman: `${installCmd("pacman", "qemu libvirt virt-manager")}; systemctl enable --now libvirtd`,
          zypper: `${installCmd("zypper", "qemu-kvm libvirt virt-install virt-manager")}; systemctl enable --now libvirtd`,
        }) },
    ],
  },
  {
    id: "gpu", name: "GPU Drivers", icon: "🎮",
    tasks: [
      { id: "gpu-status", name: "GPU / driver status", desc: "lspci, nvidia-smi and Vulkan summary", group: "Info", root: false,
        cmd: "lspci -nnk | grep -EA3 'VGA|3D|Display'; echo; nvidia-smi 2>&1 | head -n 20; echo; vulkaninfo --summary 2>/dev/null | head -n 20" },
      { id: "sb-state", name: "Secure Boot status", desc: "NVIDIA modules need an enrolled key when Secure Boot is on", group: "Info", root: false,
        cmd: "mokutil --sb-state 2>&1 || echo 'mokutil is not installed'" },
      { id: "nv-auto", name: "Install recommended NVIDIA driver", desc: "Detects your GPU generation and picks the right driver branch (Pascal → 580xx legacy)", group: "NVIDIA", reboot: true, cmd: nvAuto },
      { id: "nv-580", name: "NVIDIA 580xx (Maxwell / Pascal / Volta)", desc: "GTX 750–10xx, GTX 9xx, Titan X/Xp, Quadro P/M, Tesla V100. Last branch supporting these cards", group: "NVIDIA", reboot: true,
        cmd: (ctx) => (ctx.pm === "pacman" ? archLegacy("580xx")(ctx) : nvidia("580xx")(ctx)) },
      { id: "nv-current", name: "NVIDIA current (Turing and newer)", desc: "GTX 16xx, RTX 20/30/40/50 series", group: "NVIDIA", reboot: true, cmd: nvidia("current") },
      { id: "nv-470", name: "NVIDIA 470xx (Kepler)", desc: "GTX 600/700 series (older, X11 only)", group: "NVIDIA", reboot: true,
        cmd: (ctx) => (ctx.pm === "pacman" ? archLegacy("470xx")(ctx) : nvidia("470xx")(ctx)) },
      { id: "nv-390", name: "NVIDIA 390xx (Fermi)", desc: "GTX 400/500 series (Fedora only, X11 only)", group: "NVIDIA", reboot: true, cmd: nvidia("390xx") },
      { id: "nv-modeset", name: "Enable nvidia-drm modeset", desc: "Recommended for Wayland and smoother login", group: "NVIDIA tweaks", reboot: true,
        cmd: (ctx) => `echo 'options nvidia-drm modeset=1' > /etc/modprobe.d/nvidia-drm-modeset.conf && ${REBUILD_INITRAMFS[ctx.pm] || "true"}` },
      { id: "nv-rebuild", name: "Rebuild NVIDIA module (Fedora)", desc: "Force akmods to rebuild after a kernel update", group: "NVIDIA tweaks", reboot: true,
        cmd: (ctx) => (ctx.fedora ? "akmods --force && dracut --force" : undefined) },
      { id: "nv-sb", name: "Secure Boot: key enrollment help", desc: "Shows the steps to enroll the akmods key (needs a terminal password prompt)", group: "NVIDIA tweaks", root: false,
        cmd: "echo 'Run in a terminal:'; echo '  sudo kmodgenca -a'; echo '  sudo mokutil --import /etc/pki/akmods/certs/public_key.der'; echo 'Set a one-time password, reboot, choose Enroll MOK, enter the password.'; echo 'Alternatively disable Secure Boot in firmware setup.'" },
      { id: "nv-remove", name: "Remove NVIDIA drivers", desc: "Uninstall proprietary NVIDIA packages (falls back to nouveau)", group: "NVIDIA tweaks", danger: true, reboot: true,
        cmd: by({ dnf: "dnf remove -y '*nvidia*' --exclude=nvidia-gpu-firmware; dracut --force", apt: "apt-get remove --purge -y '^nvidia-.*' && apt-get autoremove -y", pacman: "pacman -Rns --noconfirm $(pacman -Qq | grep -E '^nvidia')" }) },
      { id: "amd", name: "AMD graphics stack", desc: "Mesa, Vulkan (RADV), VA-API tools", group: "AMD / Intel",
        cmd: inst("mesa-vulkan-drivers vulkan-tools libva-utils", { apt: "mesa-vulkan-drivers mesa-va-drivers vulkan-tools vainfo", pacman: "mesa vulkan-radeon libva-mesa-driver vulkan-tools libva-utils", zypper: "Mesa-dri vulkan-tools libva-utils" }) },
      { id: "intel", name: "Intel graphics stack", desc: "Media driver, Vulkan, VA-API tools", group: "AMD / Intel",
        cmd: inst("intel-media-driver vulkan-tools libva-utils", { apt: "intel-media-va-driver mesa-vulkan-drivers vulkan-tools vainfo", pacman: "intel-media-driver vulkan-intel vulkan-tools libva-utils", zypper: "intel-media-driver vulkan-tools libva-utils" }) },
      { id: "vulkan-tools", name: "Vulkan + OpenGL tools", desc: "vulkaninfo, glxinfo, vkcube", group: "Tools",
        cmd: inst("vulkan-tools mesa-demos", { apt: "vulkan-tools mesa-utils", pacman: "vulkan-tools mesa-utils", zypper: "vulkan-tools Mesa-demo-x" }) },
    ],
  },
  {
    id: "apps", name: "Applications", icon: "📦",
    tasks: [
      { id: "firefox", name: "Firefox", desc: "Web browser", group: "Browsers", cmd: inst("firefox") },
      { id: "chromium", name: "Chromium", desc: "Web browser", group: "Browsers", cmd: inst("chromium") },
      { id: "brave", name: "Brave", desc: "Privacy browser (official Brave repo script; Fedora, Ubuntu/Debian, openSUSE)", group: "Browsers",
        cmd: by({
          dnf: "curl -fsS https://dl.brave.com/install.sh | sh",
          apt: "curl -fsS https://dl.brave.com/install.sh | sh",
          zypper: "curl -fsS https://dl.brave.com/install.sh | sh",
        }) },
      { id: "zen", name: "Zen Browser (Flatpak)", desc: "Firefox-based browser focused on privacy and customization", group: "Browsers", ...flatpak("app.zen_browser.zen") },
      { id: "git", name: "Git", desc: "Version control", group: "Development", cmd: inst("git") },
      { id: "node", name: "Node.js + npm", desc: "JavaScript runtime", group: "Development", cmd: inst("nodejs npm") },
      { id: "python", name: "Python 3 + pip", desc: "Python runtime", group: "Development", cmd: inst("python3 python3-pip", { pacman: "python python-pip" }) },
      { id: "docker", name: "Docker", desc: "Container engine (adds you to the docker group)", group: "Development",
        cmd: by({
          dnf: `${installCmd("dnf", "moby-engine docker-compose")} && systemctl enable --now docker && usermod -aG docker $(id -nu $PKEXEC_UID)`,
          apt: `${installCmd("apt", "docker.io docker-compose")} && systemctl enable --now docker && usermod -aG docker $(id -nu $PKEXEC_UID)`,
          pacman: `${installCmd("pacman", "docker docker-compose")} && systemctl enable --now docker && usermod -aG docker $(id -nu $PKEXEC_UID)`,
        }) },
      { id: "vscode", name: "Visual Studio Code", desc: "Code editor (official Microsoft repo)", group: "Development",
        cmd: by({
          dnf: [
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
          ].join("\n"),
          apt: [
            "export DEBIAN_FRONTEND=noninteractive",
            "apt-get install -y wget gpg",
            "wget -qO- https://packages.microsoft.com/keys/microsoft.asc | gpg --dearmor > /usr/share/keyrings/microsoft.gpg",
            "echo 'deb [arch=amd64,arm64,armhf signed-by=/usr/share/keyrings/microsoft.gpg] https://packages.microsoft.com/repos/code stable main' > /etc/apt/sources.list.d/vscode.list",
            "apt-get update",
            "apt-get install -y code",
          ].join("\n"),
        }) },
      { id: "vscode-flatpak", name: "Visual Studio Code (Flatpak)", desc: "Works on every distro, including Arch", group: "Development", ...flatpak("com.visualstudio.code") },
      { id: "zed", name: "Zed", desc: "Fast code editor (official install script, installs to ~/.local)", group: "Development", root: false,
        cmd: "curl -f https://zed.dev/install.sh | sh" },
      { id: "vlc", name: "VLC", desc: "Media player", group: "Media", cmd: inst("vlc") },
      { id: "mpv", name: "mpv", desc: "Lightweight media player", group: "Media", cmd: inst("mpv") },
      { id: "obs", name: "OBS Studio", desc: "Recording and streaming (Fedora: needs RPM Fusion)", group: "Media", cmd: inst("obs-studio") },
      { id: "steam", name: "Steam", desc: "Fedora: needs RPM Fusion. Arch: needs multilib. Ubuntu: multiverse", group: "Gaming",
        cmd: inst("steam", { apt: "steam-installer" }) },
      { id: "lutris", name: "Lutris", desc: "Game launcher", group: "Gaming", cmd: inst("lutris") },
      { id: "mangohud", name: "MangoHud", desc: "FPS / performance overlay", group: "Gaming", cmd: inst("mangohud") },
      { id: "wine", name: "Wine", desc: "Run Windows apps", group: "Gaming", cmd: inst("wine") },
      { id: "discord", name: "Discord (Flatpak)", desc: "Chat", group: "Flatpak apps", ...flatpak("com.discordapp.Discord") },
      { id: "spotify", name: "Spotify (Flatpak)", desc: "Music", group: "Flatpak apps", ...flatpak("com.spotify.Client") },
      { id: "heroic", name: "Heroic Launcher (Flatpak)", desc: "Epic / GOG games", group: "Flatpak apps", ...flatpak("com.heroicgameslauncher.hgl") },
      { id: "htop", name: "htop", desc: "Process viewer", group: "Terminal tools", cmd: inst("htop") },
      { id: "btop", name: "btop", desc: "Resource monitor", group: "Terminal tools", cmd: inst("btop") },
      { id: "fastfetch", name: "fastfetch", desc: "System info in the terminal", group: "Terminal tools", cmd: inst("fastfetch") },
      { id: "neovim", name: "Neovim", desc: "Text editor", group: "Terminal tools", cmd: inst("neovim") },
      { id: "tmux", name: "tmux", desc: "Terminal multiplexer", group: "Terminal tools", cmd: inst("tmux") },
     /* Install DigiDoc4 and Web EID */
      /* FileZilla */
      { id: "filezilla", name: "FileZilla", desc: "FTP client (Homebrew: via Flatpak)", group: "Tools",
        cmd: (ctx) => ctx.pm === "brew"
          ? flatpak("org.filezillaproject.Filezilla").cmd
          : inst("filezilla", { emerge: "net-ftp/filezilla" })(ctx) },
      /* NGINX */
      { id: "nginx", name: "NGINX web server", desc: "Install and start NGINX", group: "Web tools", sys: true,
        cmd: (ctx) => { const i = installCmd(ctx.pm, NGINX(ctx.pm)); return i && join(i, svc("nginx")); } },
      /* PHP complete stack */
      { id: "php", name: "PHP complete stack", desc: "Install PHP-FPM, MariaDB and NGINX", group: "Web tools", sys: true,
        cmd: (ctx) => {
          const p = PHP_PKGS[ctx.pm], d = DB[ctx.pm];
          if (!p || !d) return undefined;
          const use = ctx.pm === "emerge" ? 'USE="fpm mysql mysqli pdo" ' : "";
          return join(use + installCmd(ctx.pm, `${p} ${NGINX(ctx.pm)} ${d[0]}`), d[1], PHP_SETUP[ctx.pm], phpSvc(ctx.pm), svc("nginx"), svc(d[2]));
        } },
      /* MySQL / MariaDB */
      { id: "mysql", name: "MySQL / MariaDB", desc: "Install and start the MariaDB server (MySQL compatible)", group: "Web tools", sys: true,
        cmd: (ctx) => { const d = DB[ctx.pm]; return d && join(installCmd(ctx.pm, d[0]), d[1], svc(d[2])); } },
      /* MongoDB */
      { id: "mongodb", name: "MongoDB", desc: "Official MongoDB Community repo: Fedora/RHEL, Ubuntu/Debian, openSUSE. Arch via AUR", group: "Web tools", sys: true,
        cmd: (ctx) => ({
          dnf: `${MONGO_DNF}\n${svc("mongod")}`,
          apt: `${MONGO_APT}\n${svc("mongod")}`,
          zypper: `${MONGO_ZYPPER}\n${svc("mongod")}`,
          yay: join(installCmd("yay", "mongodb-bin"), svc("mongodb")),
          paru: join(installCmd("paru", "mongodb-bin"), svc("mongodb")),
        })[ctx.pm] },
            /* MongoDB Compass */
      { id: "mongodb-compass", name: "MongoDB Compass", desc: `Official MongoDB GUI ${COMPASS_VER} (from mongodb.com; x86_64 only). Arch via AUR`, group: "Web tools", sys: true,
        cmd: (ctx) => {
          const base = "https://downloads.mongodb.com/compass";
          const rpm = `${base}/mongodb-compass-${COMPASS_VER}.x86_64.rpm`;
          const deb = `${base}/mongodb-compass_${COMPASS_VER}_amd64.deb`;
          const x64 = "[ \"$(uname -m)\" = x86_64 ] || { echo 'MongoDB Compass is only available for x86_64.'; exit 1; }";
          return ({
            dnf: ["set -e", x64, `dnf install -y ${rpm}`].join("\n"),
            zypper: ["set -e", x64, `zypper --non-interactive --no-gpg-checks install ${rpm}`].join("\n"),
            apt: [
              "set -e",
              "export DEBIAN_FRONTEND=noninteractive",
              x64,
              "apt-get update && apt-get install -y curl ca-certificates",
              `curl -fSL -o /tmp/mongodb-compass.deb ${deb}`,
              "apt-get install -y /tmp/mongodb-compass.deb",
              "rm -f /tmp/mongodb-compass.deb",
            ].join("\n"),
            yay: installCmd("yay", "mongodb-compass"),
            paru: installCmd("paru", "mongodb-compass"),
          })[ctx.pm];
        } },
      { id: "digidoc", name: "DigiDoc4 / Web EID", desc: "Estonian ID card / Smart ID / Mobile ID", group: "Tools",
      cmd: by({
        dnf:    `${installCmd("dnf", "qdigidoc pcsc-lite pcsc-lite-ccid opensc")} && sudo systemctl enable --now pcscd.socket; ${installCmd("dnf", "web-eid")} || echo "web-eid not in repos: install the Web eID browser extension instead"`,
        apt:    `${installCmd("apt", "qdigidoc4 pcscd pcsc-tools opensc")} && sudo systemctl enable --now pcscd.socket; ${installCmd("apt", "web-eid")} || echo "web-eid not in repos: install the Web eID browser extension instead"`,
        pacman: `${installCmd("pacman", "pcsclite ccid opensc")} && sudo systemctl enable --now pcscd.socket; ${installCmd("pacman", "qdigidoc4")} || echo "qdigidoc4 not in official repos: install it from the AUR"`,
        zypper: `${installCmd("zypper", "pcsc-lite pcsc-ccid opensc")} && sudo systemctl enable --now pcscd.socket; ${installCmd("zypper", "qdigidoc4")} || echo "qdigidoc4 not in default repos: add the RIA/OBS repository"`,
      }) },
      /* Communications Apps */
      { id: "thunderbird", name: "Thunderbird", desc: "Email client", group: "Communications", cmd: inst("thunderbird") },
      { id: "signal", name: "Signal", desc: "Secure messaging app", group: "Communications",
        cmd: by({
          dnf: `${installCmd("dnf", "signal-desktop")}`,
          apt: [
            "export DEBIAN_FRONTEND=noninteractive",
            "apt-get install -y wget gpg",
            "wget -qO- https://updates.signal.org/desktop/apt/keys.asc | gpg --dearmor > /usr/share/keyrings/signal-desktop-keyring.gpg",
            "echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/signal-desktop-keyring.gpg] https://updates.signal.org/desktop/apt xenial main' > /etc/apt/sources.list.d/signal-xenial.list",
            "apt-get update && apt-get install -y signal-desktop",
          ].join("\n"),
          pacman: `${installCmd("pacman", "signal-desktop")}`,
          zypper: `${installCmd("zypper", "signal-desktop")}`,
        }) },
      { id: "slack", name: "Slack", desc: "Team chat app", group: "Communications",
        cmd: by({
          dnf: `${installCmd("dnf", "slack")}`,
          apt: [
            "export DEBIAN_FRONTEND=noninteractive",
            "apt-get install -y wget gpg",
            "wget -qO- https://packagecloud.io/slacktechnologies/slack/gpgkey | gpg --dearmor > /usr/share/keyrings/slack-keyring.gpg",
            "echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/slack-keyring.gpg] https://packagecloud.io/slacktechnologies/slack/debian/ jessie main' > /etc/apt/sources.list.d/slack.list",
            "apt-get update && apt-get install -y slack-desktop",
          ].join("\n"),
          pacman: `${installCmd("pacman", "slack-desktop")}`,
          zypper: `${installCmd("zypper", "slack")}`,
        }) },
        { id: "discord", name: "Discord", desc: "Chat app for communities", group: "Communications",
          cmd: by({
            dnf: `${installCmd("dnf", "discord")}`,
            apt: [
              "export DEBIAN_FRONTEND=noninteractive",
              "apt-get install -y wget gpg",
              "wget -qO- https://discordapp.com/api/download?platform=linux&format=deb | gpg --dearmor > /usr/share/keyrings/discord-keyring.gpg",
              "echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/discord-keyring.gpg] https://discordapp.com/api/download?platform=linux&format=deb stable main' > /etc/apt/sources.list.d/discord.list",
              "apt-get update && apt-get install -y discord",
            ].join("\n"),
            pacman: `${installCmd("pacman", "discord")}`,
            zypper: `${installCmd("zypper", "discord")}`,
          }) },
    ],
  },
  {
    id: "security", name: "Security", icon: "🛡️",
    tasks: [
      { id: "firewall", sys: true, name: "Enable firewall", desc: "firewalld on Fedora/openSUSE, ufw elsewhere", group: "Protection",
        cmd: (ctx) => ctx.pm === "dnf" || ctx.pm === "zypper"
          ? `${installCmd(ctx.pm, "firewalld")} && systemctl enable --now firewalld && firewall-cmd --state`
          : `${installCmd(ctx.pm, "ufw")} && ufw default deny incoming && ufw default allow outgoing && ufw --force enable && ufw status` },
      { id: "fail2ban", sys: true, name: "Fail2ban", desc: "Ban IPs with repeated failed logins", group: "Protection",
        cmd: (ctx) => `${installCmd(ctx.pm, "fail2ban")} && systemctl enable --now fail2ban` },
      { id: "autoupdate", name: "Automatic security updates", desc: "dnf-automatic / unattended-upgrades", group: "Protection",
        cmd: by({ dnf: `${installCmd("dnf", "dnf-automatic")} && systemctl enable --now dnf-automatic-install.timer`, apt: installCmd("apt", "unattended-upgrades") }) },
      { id: "clamav", name: "ClamAV", desc: "Antivirus scanner", group: "Protection", cmd: inst("clamav", { dnf: "clamav clamav-update" }) },
      { id: "selinux", name: "SELinux status", desc: "Show SELinux mode (Fedora)", group: "Info", root: false, cmd: "getenforce 2>&1 || sestatus 2>&1 || echo 'SELinux not available'" },
      { id: "fw-status", name: "Firewall status", desc: "Show firewall state", group: "Info", root: false,
        cmd: "(firewall-cmd --state 2>&1) || (ufw status 2>&1) || echo 'No firewall found'" },
      { id: "listening", name: "Listening ports", desc: "Open network ports and their programs", group: "Info", root: false, cmd: "ss -tulpn 2>&1 | head -n 40" },
    ],
  },
  {
    id: "utils", name: "Utilities", icon: "🧰",
    tasks: [
      { id: "sysinfo", name: "System info", desc: "Kernel, CPU, memory, disks", group: "Info", root: false,
        cmd: "uname -a; echo; lscpu | head -n 16; echo; free -h; echo; df -h -x tmpfs -x devtmpfs" },
      { id: "disk-usage", name: "Biggest folders in home", desc: "Top 15 largest folders", group: "Info", root: false,
        cmd: 'du -h --max-depth=1 "$HOME" 2>/dev/null | sort -hr | head -n 15' },
      { id: "top-mem", name: "Top memory processes", desc: "10 biggest RAM users", group: "Info", root: false, cmd: "ps aux --sort=-%mem | head -n 11" },
      { id: "failed", name: "Failed services", desc: "systemd units in failed state", group: "Info", root: false, cmd: "systemctl --failed --no-pager" },
      { id: "boot-time", name: "Boot time analysis", desc: "Slowest startup services", group: "Info", root: false, cmd: "systemd-analyze; echo; systemd-analyze blame | head -n 15" },
      { id: "smart", sys: true, name: "Disk health (SMART)", desc: "Install smartmontools and scan drives", group: "Hardware",
        cmd: (ctx) => `${installCmd(ctx.pm, "smartmontools")} && smartctl --scan && for d in $(smartctl --scan | awk '{print $1}'); do smartctl -H $d; done` },
      { id: "net-info", name: "Network info", desc: "IP addresses and DNS", group: "Network", root: false, cmd: "ip -br a; echo; resolvectl status 2>/dev/null | head -n 20" },
      { id: "restart-net", name: "Restart network", desc: "Restart NetworkManager", group: "Network", cmd: "systemctl restart NetworkManager" },
      { id: "flatpak-update", name: "Update Flatpaks", desc: "Update all Flatpak apps", group: "Maintenance", root: false, cmd: "flatpak update -y" },
      { id: "clear-cache", name: "Clear user cache", desc: "Delete ~/.cache contents", group: "Maintenance", root: false, danger: true, cmd: 'rm -rf "$HOME"/.cache/* && echo Done' },
    ],
  },
];