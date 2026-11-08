#!/usr/bin/env bash
# dev_setup.sh: install what the repository's local checks need on an Ubuntu
# 24.04 dev box or cloud container (docs/PLAN.md, E1; docs/PLAYBOOK.md, P1):
#   - the game crate's build libraries (the same list as CI's
#     "Install system libraries for the game crate" steps);
#   - Xvfb with Mesa's lavapipe and the X libraries winit loads, for the
#     windowed checks (scripts/render_smoke.sh and friends), and xdotool for
#     scripts/art_drive.sh;
#   - the pinned toolchain from rust-toolchain.toml;
#   - the cargo tools `just ci` runs: just, typos, cargo-machete, cargo-deny
#     (cargo-nextest is CI-only; `just ci` falls back to `cargo test`).
# Idempotent: anything already present is skipped. macOS developers use
# Homebrew instead (docs/HANDOFF.md, "Local setup").
#
# Usage: scripts/dev_setup.sh   (as root, or with sudo available)
set -euo pipefail
cd "$(dirname "$0")/.."

if [ "$(uname -s)" != "Linux" ]; then
  echo "dev_setup: Linux only (on macOS: brew install just cargo-nextest cargo-deny typos-cli)" >&2
  exit 1
fi

sudo=""
if [ "$(id -u)" -ne 0 ]; then sudo="sudo"; fi

pkgs="libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev xvfb mesa-vulkan-drivers libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxi6 libxrandr2 xdotool"
missing=""
for p in $pkgs; do
  dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"
done
if [ -n "$missing" ]; then
  echo "dev_setup: apt-get install$missing"
  $sudo apt-get update -qq
  # shellcheck disable=SC2086 # the package list is meant to split
  DEBIAN_FRONTEND=noninteractive $sudo apt-get install -y -qq --no-install-recommends $missing
fi

# Installs the toolchain rust-toolchain.toml pins, if it is missing.
rustup show active-toolchain

for pair in just:just typos:typos-cli cargo-machete:cargo-machete cargo-deny:cargo-deny; do
  bin="${pair%%:*}"
  crate="${pair#*:}"
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "dev_setup: cargo install --locked $crate"
    cargo install --locked "$crate"
  fi
done

echo "dev_setup: ok"
