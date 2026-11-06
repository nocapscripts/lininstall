#!/usr/bin/env bash
# LinInstall (Tauri) build script
# Usage: ./build.sh [rpm|deb|appimage|all] [--install]
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

TARGET="${1:-rpm}"
INSTALL=false
[[ "${2:-}" == "--install" || "${1:-}" == "--install" ]] && INSTALL=true
[[ "$TARGET" == "--install" ]] && TARGET="rpm"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31mError: %s\033[0m\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "Rust is missing (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh)"
command -v node  >/dev/null || die "node is missing (sudo dnf install nodejs npm)"
command -v npm   >/dev/null || die "npm is missing (sudo dnf install nodejs npm)"
[[ -f src-tauri/icons/icon.png ]] || die "src-tauri/icons/icon.png not found"

if [[ ! -d node_modules ]]; then
  say "Installing dependencies"
  npm install
fi

case "$TARGET" in
  rpm)      say "Building rpm";      npm run build:rpm ;;
  deb)      say "Building deb";      npm run build:deb ;;
  appimage) say "Building AppImage"; npm run build:appimage ;;
  all)      say "Building all";      npm run build ;;
  *) die "unknown target '$TARGET' (use rpm, deb, appimage or all)" ;;
esac

OUT="src-tauri/target/release/bundle"
say "Done. Packages:"
ls -lh "$OUT"/*/ 2>/dev/null | grep -E '\.(rpm|deb|AppImage)$' || true

if $INSTALL; then
  RPM_FILE="$(ls "$OUT"/rpm/*.rpm 2>/dev/null | head -n1 || true)"
  [[ -n "$RPM_FILE" ]] || die "no rpm found to install (build with: ./build.sh rpm --install)"
  say "Installing rpm"
  sudo dnf reinstall -y "./$RPM_FILE" || sudo dnf install -y "./$RPM_FILE"
  sudo gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true
  sudo update-desktop-database 2>/dev/null || true
fi
