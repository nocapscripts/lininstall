#!/usr/bin/env bash
# LinInstall build script
# Usage: ./build.sh [rpm|appimage|deb|all] [--install]
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"   # always run from the project root

TARGET="${1:-rpm}"
INSTALL=false
[[ "${2:-}" == "--install" || "${1:-}" == "--install" ]] && INSTALL=true
[[ "$TARGET" == "--install" ]] && TARGET="rpm"

say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
die() { printf '\033[1;31mError: %s\033[0m\n' "$*" >&2; exit 1; }

# ---- checks ----
command -v node >/dev/null || die "node is not installed (sudo dnf install nodejs npm)"
command -v yarn >/dev/null || die "yarn is not installed (sudo npm install -g yarn)"
[[ -f package.json ]] || die "package.json not found"
[[ -f build/icon.png ]] || die "build/icon.png not found (512x512 PNG needed)"

# ---- dependencies ----
if [[ ! -d node_modules ]]; then
  say "Installing dependencies"
  yarn install
fi

# ---- icons (sized, so packages don't end up with a 0x0 icon folder) ----
if [[ ! -f build/icons/512x512.png ]]; then
  say "Generating icon sizes"
  command -v magick >/dev/null || die "ImageMagick missing (sudo dnf install ImageMagick)"
  mkdir -p build/icons
  for s in 16 24 32 48 64 128 256 512; do
    magick build/icon.png -resize "${s}x${s}" "build/icons/${s}x${s}.png"
  done
fi
grep -q '"icon": "build/icons"' package.json \
  || echo 'Warning: set "icon": "build/icons" under build.linux in package.json'

# ---- rpm needs rpmbuild + a current fpm ----
if [[ "$TARGET" == "rpm" || "$TARGET" == "all" ]]; then
  command -v rpmbuild >/dev/null || die "rpmbuild missing (sudo dnf install rpm-build)"
  command -v fpm >/dev/null || die "fpm missing (sudo dnf install ruby ruby-devel gcc make redhat-rpm-config && sudo gem install fpm)"
  export USE_SYSTEM_FPM=true
fi
if [[ "$TARGET" == "deb" ]]; then export USE_SYSTEM_FPM=true; fi

# ---- build ----
say "Cleaning dist/"
/bin/rm -rf dist

case "$TARGET" in
  rpm)      say "Building rpm";      yarn dist:rpm ;;
  appimage) say "Building AppImage"; yarn dist:appimage ;;
  deb)      say "Building deb";      yarn dist:deb ;;
  all)      say "Building all";      yarn dist ;;
  *) die "unknown target '$TARGET' (use rpm, appimage, deb or all)" ;;
esac

# ---- verify rpm icon paths ----
RPM_FILE="$(ls dist/*.rpm 2>/dev/null | head -n1 || true)"
if [[ -n "$RPM_FILE" ]]; then
  say "Icons inside $RPM_FILE"
  rpm -qlp "$RPM_FILE" | grep -E 'icons|\.desktop' || true
  if rpm -qlp "$RPM_FILE" | grep -q '0x0'; then
    echo "Warning: package still contains a 0x0 icon folder"
  fi
fi

say "Done. Files in dist/:"
ls -lh dist | grep -E '\.(rpm|deb|AppImage)$' || true

# ---- optional install ----
if $INSTALL && [[ -n "$RPM_FILE" ]]; then
  say "Installing rpm"
  sudo dnf reinstall -y "./$RPM_FILE" || sudo dnf install -y "./$RPM_FILE"
  sudo gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true
  sudo update-desktop-database 2>/dev/null || true
  say "Installed. Log out/in if the icon doesn't show yet."
fi
