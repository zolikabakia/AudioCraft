#!/usr/bin/env bash
# Build a Linux x86_64 release of AudioCraft and place it in dist/.
# Run from any folder:  bash scripts/build-linux.sh
# Add --no-deps when the system libraries are already installed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is not installed. Install it from https://rustup.rs then run this again." >&2
  exit 1
fi

install_deps() {
  SUDO=""
  if [ "${EUID:-$(id -u)}" -ne 0 ]; then
    if ! command -v sudo >/dev/null 2>&1; then
      echo "System libraries may be missing and sudo is not available. Install them as root, then run:" >&2
      echo "  bash scripts/build-linux.sh --no-deps" >&2
      echo "Debian/Ubuntu packages: build-essential pkg-config libasound2-dev libgtk-3-dev libx11-dev libxcb1-dev libxkbcommon-dev libxrandr-dev libxi-dev libwayland-dev libgl1-mesa-dev libfontconfig1-dev libudev-dev" >&2
      return 0
    fi
    # sudo asks for the password when it needs one
    SUDO=sudo
  fi
  if command -v apt-get >/dev/null 2>&1; then
    $SUDO apt-get update
    $SUDO apt-get install -y build-essential pkg-config libasound2-dev libgtk-3-dev \
      libx11-dev libxcb1-dev libxkbcommon-dev libxrandr-dev libxi-dev \
      libwayland-dev libgl1-mesa-dev libfontconfig1-dev libudev-dev
  elif command -v dnf >/dev/null 2>&1; then
    $SUDO dnf install -y gcc make pkgconf-pkg-config alsa-lib-devel gtk3-devel \
      libX11-devel libxcb-devel libxkbcommon-devel libXrandr-devel libXi-devel \
      wayland-devel mesa-libGL-devel fontconfig-devel systemd-devel
  elif command -v pacman >/dev/null 2>&1; then
    $SUDO pacman -S --needed base-devel pkgconf alsa-lib gtk3 libx11 libxcb \
      libxkbcommon libxrandr libxi wayland mesa fontconfig systemd
  else
    echo "Unknown package manager; install the X11/Wayland, ALSA, GTK and fontconfig development packages manually." >&2
  fi
}

if [ "${1:-}" != "--no-deps" ]; then
  install_deps
fi

cargo build --release --locked

mkdir -p dist
cp target/release/audiocraft dist/audiocraft-linux-x86_64
chmod +x dist/audiocraft-linux-x86_64
cp README.md LICENSE dist/
# the same icon every build uses, and a desktop entry (copy both to ~/.local/share/... to install)
cp target/release/audiocraft.png target/release/audiocraft.desktop dist/ 2>/dev/null || true
if [ -f docs/COMMANDS.md ]; then cp docs/COMMANDS.md dist/; fi
tar -czf dist/audiocraft-linux-x86_64.tar.gz -C dist audiocraft-linux-x86_64 README.md LICENSE \
  $( [ -f dist/audiocraft.png ] && echo audiocraft.png audiocraft.desktop || true ) \
  $( [ -f dist/COMMANDS.md ] && echo COMMANDS.md || true )

echo
echo "Linux build ready: $ROOT/dist/audiocraft-linux-x86_64"
echo "Run it with: ./dist/audiocraft-linux-x86_64"
