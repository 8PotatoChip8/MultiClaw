#!/usr/bin/env bash
# MultiClaw Stable Release Installer
# This script resolves the latest GitHub release and installs that version.
# For development builds, use install.sh directly instead.
set -euo pipefail

echo "[multiclaw] Fetching latest stable release..."

LATEST=$(curl -fsSL https://api.github.com/repos/8PotatoChip8/MultiClaw/releases/latest | grep -oP '"tag_name":\s*"\K[^"]+')

if [ -z "$LATEST" ]; then
  echo "[multiclaw] ERROR: Could not determine latest release from GitHub."
  echo "[multiclaw] Check https://github.com/8PotatoChip8/MultiClaw/releases"
  exit 1
fi

echo "[multiclaw] Installing MultiClaw $LATEST (stable release)..."
export MULTICLAW_VERSION="$LATEST"
exec bash <(curl -fsSL "https://raw.githubusercontent.com/8PotatoChip8/MultiClaw/$LATEST/infra/install/install.sh")
