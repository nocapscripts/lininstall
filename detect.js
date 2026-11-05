const { execSync } = require("child_process");
const os = require("os");
const fs = require("fs");

const sh = (cmd) => {
  try { return execSync(cmd, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 8000 }).trim(); }
  catch { return ""; }
};

const GEN = {
  GP: ["Pascal", "580xx"], GM: ["Maxwell", "580xx"], GV: ["Volta", "580xx"],
  GK: ["Kepler", "470xx"], GF: ["Fermi", "390xx"],
  TU: ["Turing", "current"], GA: ["Ampere", "current"], AD: ["Ada Lovelace", "current"], GB: ["Blackwell", "current"],
};

function nvidiaGeneration(name) {
  const m = name.match(/\b(GP|GM|GV|GK|GF|TU|GA|AD|GB)\d{2,3}/);
  if (m) return GEN[m[1]];
  // Fallback when lspci only shows the marketing name
  if (/GTX\s*10\d\d|GT\s*10[34]0|TITAN X|TITAN Xp|Quadro P\d{3,4}|MX1[5-9]0/i.test(name)) return GEN.GP;
  if (/GTX\s*9\d\d|GTX\s*75\d|GTX\s*TITAN X|Quadro M\d{3,4}|MX1[1-4]0/i.test(name)) return GEN.GM;
  if (/GTX\s*[67]\d\d|GT\s*[67]\d\d|Quadro K/i.test(name)) return GEN.GK;
  if (/RTX|GTX\s*16\d\d|Quadro RTX/i.test(name)) return GEN.TU;
  return [null, null];
}

function parseGpu(line) {
  const name = line.replace(/^\S+\s+[^:]+\[\w+\]:\s*/, "").replace(/\s*\[[0-9a-f]{4}:[0-9a-f]{4}\].*$/i, "");
  const vendor = /\bNVIDIA\b/i.test(name) ? "NVIDIA" : /\bIntel\b/i.test(name) ? "Intel" : /\bAMD\b|\bATI\b|Advanced Micro Devices/.test(name) ? "AMD" : "Other";
  const gpu = { name, vendor, generation: null, branch: null };
  if (vendor === "NVIDIA") [gpu.generation, gpu.branch] = nvidiaGeneration(name);
  return gpu;
}

function detect() {
  const rel = {};
  try {
    fs.readFileSync("/etc/os-release", "utf8").split("\n").forEach((l) => {
      const i = l.indexOf("=");
      if (i > 0) rel[l.slice(0, i)] = l.slice(i + 1).replace(/^"|"$/g, "");
    });
  } catch {}
  const idLike = `${rel.ID || ""} ${rel.ID_LIKE || ""}`;
  const has = (b) => !!sh(`command -v ${b}`);
  // Linuxbrew is often not on PATH for GUI apps, so add its usual locations
  [ "/home/linuxbrew/.linuxbrew/bin", `${os.homedir()}/.linuxbrew/bin` ].forEach((d) => {
    if (fs.existsSync(`${d}/brew`) && !(process.env.PATH || "").split(":").includes(d)) process.env.PATH = `${d}:${process.env.PATH}`;
  });
  // [binary, name] - system managers first so they win the fallback
  const available = [["dnf", "dnf"], ["apt-get", "apt"], ["pacman", "pacman"], ["zypper", "zypper"],
    ["apk", "apk"], ["xbps-install", "xbps"], ["emerge", "emerge"],
    ["yay", "yay"], ["paru", "paru"], ["nix", "nix"], ["brew", "brew"]]
    .filter(([bin]) => has(bin)).map(([, name]) => name);
  // The system's own package manager, based on the distro family
  const family = /fedora|rhel|centos|rocky|alma/i.test(idLike) ? "dnf"
    : /debian|ubuntu|mint|pop/i.test(idLike) ? "apt"
    : /arch|manjaro|endeavour|cachyos/i.test(idLike) ? "pacman"
    : /suse/i.test(idLike) ? "zypper"
    : /alpine/i.test(idLike) ? "apk"
    : /void/i.test(idLike) ? "xbps"
    : /gentoo/i.test(idLike) ? "emerge"
    : /nixos/i.test(idLike) ? "nix" : null;
  const defaultPm = available.includes(family) ? family : available[0] || null;
  const pm = defaultPm;

  const gpuLines = sh("lspci -nn").split("\n").filter((l) => /VGA compatible|3D controller|Display controller/i.test(l));
  const gpus = gpuLines.map(parseGpu);
  const nvidia = gpus.find((g) => g.vendor === "NVIDIA") || null;

  const cpus = os.cpus();
  return {
    pm,
    defaultPm,
    availablePms: available,
    allPms: ["dnf", "apt", "pacman", "zypper", "apk", "xbps", "emerge", "yay", "paru", "nix", "brew"],
    pmChoice: "auto",
    fedora: /fedora/i.test(idLike),
    ubuntuLike: /ubuntu/i.test(idLike),
    distro: rel.PRETTY_NAME || "Linux",
    hostname: os.hostname(),
    kernel: os.release(),
    cpu: cpus.length ? `${cpus[0].model} (${cpus.length} threads)` : "unknown",
    ramGB: (os.totalmem() / 1024 ** 3).toFixed(1),
    session: process.env.XDG_SESSION_TYPE || "unknown",
    desktop: process.env.XDG_CURRENT_DESKTOP || "unknown",
    secureBoot: sh("mokutil --sb-state").replace(/\n.*/s, "") || "unknown (mokutil not installed)",
    nvidiaDriver: sh("nvidia-smi --query-gpu=driver_version --format=csv,noheader").split("\n")[0] || null,
    gpus,
    nvidia, // first NVIDIA GPU or null
    hasLspci: has("lspci"),
  };
}

module.exports = { detect };
