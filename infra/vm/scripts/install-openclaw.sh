#!/usr/bin/env bash
# MultiClaw — Custom OpenClaw Agent Runtime Installer
# This script is run inside each agent VM via cloud-init to install
# the OpenClaw runtime (https://github.com/openclaw/openclaw) and
# configure it for use as a managed MultiClaw agent.
#
# Environment variables expected:
#   AGENT_USER       — the unprivileged user to run OpenClaw under (default: employee)
#   HOST_IP          — the host machine IP for contacting multiclawd
#   MULTICLAWD_PORT  — control-plane port on the host (default: 8080)
#   OLLAMA_PROXY_PORT— ollama proxy port on the host (default: 11436)

set -euo pipefail

AGENT_USER="${AGENT_USER:-employee}"
HOST_IP="${HOST_IP:-127.0.0.1}"
MULTICLAWD_PORT="${MULTICLAWD_PORT:-8080}"
OLLAMA_PROXY_PORT="${OLLAMA_PROXY_PORT:-11436}"

log() { echo "[multiclaw-agent-setup] $*"; }

# ─── 1. Install Node.js 22 (required by OpenClaw) ────────────────────────
log "Installing Node.js 22..."
if ! command -v node &>/dev/null || [[ "$(node --version | cut -d. -f1 | tr -d v)" -lt 22 ]]; then
    curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
    apt-get install -y nodejs
fi
log "Node version: $(node --version)"

# ─── 2. Install OpenClaw from npm (official package) ─────────────────────
log "Installing OpenClaw runtime..."
npm install -g openclaw@latest
log "OpenClaw version: $(openclaw --version 2>/dev/null || echo 'installed')"

# ─── 3. Create agent workspace directories ───────────────────────────────
log "Setting up agent workspace..."
AGENT_HOME="/home/${AGENT_USER}"
mkdir -p "${AGENT_HOME}/.openclaw"
mkdir -p "${AGENT_HOME}/workspace"
mkdir -p "${AGENT_HOME}/.openclaw/skills"
mkdir -p "${AGENT_HOME}/.openclaw/logs"

# ─── 4. Set correct permissions ──────────────────────────────────────────
log "Setting permissions..."
chown -R "${AGENT_USER}:${AGENT_USER}" "${AGENT_HOME}/.openclaw"
chown -R "${AGENT_USER}:${AGENT_USER}" "${AGENT_HOME}/workspace"
chmod 700 "${AGENT_HOME}/.openclaw"
chmod 644 "${AGENT_HOME}/.openclaw/openclaw.json" 2>/dev/null || true

# ─── 5. Run openclaw onboard (non-interactive, daemon mode) ──────────────
log "Running OpenClaw onboard..."
su - "${AGENT_USER}" -c "openclaw onboard --install-daemon --non-interactive" || {
    log "WARN: openclaw onboard returned non-zero (may be expected on first run)"
}

# ─── 6. Install common tools for agent work ──────────────────────────────
log "Installing agent toolchain..."
apt-get install -y --no-install-recommends \
    git \
    python3 \
    python3-pip \
    build-essential \
    unzip \
    wget \
    2>/dev/null || true

# ─── 7. Verify installation ─────────────────────────────────────────────
log "Verifying installation..."
if command -v openclaw &>/dev/null; then
    log "✓ OpenClaw CLI installed"
else
    log "✗ OpenClaw CLI not found in PATH"
    exit 1
fi

if [ -f "${AGENT_HOME}/.openclaw/openclaw.json" ]; then
    log "✓ OpenClaw config present"
else
    log "✗ OpenClaw config missing (cloud-init should have written it)"
fi

log "OpenClaw agent runtime setup complete."
