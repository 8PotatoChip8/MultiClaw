#!/usr/bin/env bash
set -e

# MultiClaw MVP Installer Script
# Host strictly Ubuntu 24.04

log() {
  echo -e "\033[1;32m[multiclaw-install]\033[0m $1"
}

if [[ $EUID -ne 0 ]]; then
   echo "This script must be run as root (or with sudo)" 
   exit 1
fi

log "Detecting OS..."
OS_VERSION=$(grep VERSION_ID /etc/os-release | cut -d '=' -f 2 | tr -d '"')
if [[ "$OS_VERSION" != "24.04" ]]; then
  echo "WARNING: MultiClaw targets Ubuntu 24.04. You have $OS_VERSION. Proceeding at your own risk."
fi

# ── Determine the host IP early ──
HOST_IP=$(hostname -I | awk '{print $1}')
log "Detected host IP: $HOST_IP"

log "Installing dependencies (incus, qemu-kvm, docker, curl, jq, git)..."
apt-get update
apt-get install -y apt-transport-https ca-certificates curl software-properties-common jq git qemu-kvm 
apt-get install -y incus

log "Installing Docker..."
# Skip if docker is already running
if ! command -v docker &> /dev/null; then
  curl -fsSL https://get.docker.com -o get-docker.sh
  sh get-docker.sh
fi

log "Installing Ollama on Host..."
if ! command -v ollama &> /dev/null; then
  curl -fsSL https://ollama.com/install.sh | sh
fi
systemctl enable --now ollama

# ── Ollama Login for cloud models ──
log "Ollama Login (required for cloud models)..."
echo ""
echo "=============================================="
echo "  You need to log in to Ollama to use cloud"
echo "  models. Running 'ollama login' now..."
echo "=============================================="
echo ""

# Run ollama login directly — it will print a URL if no browser is available.
# We need to preserve stdin for the read below, so don't pipe through su.
ollama login </dev/tty || echo "Ollama login returned non-zero. You can retry later with: ollama login"

echo ""
echo "If you completed the login in your browser, press ENTER to continue."
echo "If you want to skip login for now, just press ENTER."
read -r -p "Press ENTER to continue... " </dev/tty

# ── Open firewall ports ──
log "Configuring firewall (allowing ports 3000, 8080)..."
if command -v ufw &> /dev/null; then
  ufw allow 3000/tcp comment "MultiClaw Dashboard" || true
  ufw allow 8080/tcp comment "MultiClaw API" || true
  ufw allow 11434/tcp comment "Ollama API" || true
fi

log "Generating Master Key..."
mkdir -p /var/lib/multiclaw
mkdir -p /opt/multiclaw

if [ ! -f /var/lib/multiclaw/master.key ]; then
  # 32 bytes hex length = 64
  head -c 32 /dev/urandom | od -A n -t x1 | tr -d ' \n' > /var/lib/multiclaw/master.key
  chmod 0600 /var/lib/multiclaw/master.key
fi

if [ ! -f /var/lib/multiclaw/admin.token ]; then
  head -c 16 /dev/urandom | od -A n -t x1 | tr -d ' \n' > /var/lib/multiclaw/admin.token
  chmod 0600 /var/lib/multiclaw/admin.token
fi

log "Cloning missing structure for local compose..."
if [ ! -d /opt/multiclaw/.git ]; then
   # We clone into /opt/multiclaw
   rm -rf /opt/multiclaw
   git clone https://github.com/8PotatoChip8/MultiClaw.git /opt/multiclaw || echo "Clone failed. Proceeding anyway..."
fi

log "Creating env file..."
ADMIN_TOKEN=$(cat /var/lib/multiclaw/admin.token)
cat > /opt/multiclaw/infra/docker/.env <<EOF
DB_URL=postgres://multiclaw:multiclaw_pass@127.0.0.1:5432/multiclaw
ADMIN_TOKEN=${ADMIN_TOKEN}
MASTER_KEY_PATH=/var/lib/multiclaw/master.key
PORT=8080
UI_PORT=3000
PROXY_PORT=11436
HOST_IP=${HOST_IP}
EOF

log "Starting compose stack (if active repo)..."
cd /opt/multiclaw
docker compose -f infra/docker/docker-compose.yml up -d --build

log "Waiting for control-plane backend to be ready..."
for i in {1..90}; do
  if curl -s http://127.0.0.1:8080/v1/health > /dev/null; then
    break
  fi
  sleep 2
done

log "Interactive Initialization..."
echo "--- MultiClaw Setup ---"

HOLDING_NAME=${HOLDING_NAME:-"Main Holding"}
MAIN_AGENT_NAME=${MAIN_AGENT_NAME:-"MainAgent"}
DEFAULT_MODEL=${DEFAULT_MODEL:-"glm-5:cloud"}
STRICT_MODE=${STRICT_MODE:-"false"}

read -r -p "Holding Name [$HOLDING_NAME]: " USER_HOLDING </dev/tty
if [[ -n "$USER_HOLDING" ]]; then HOLDING_NAME="$USER_HOLDING"; fi

read -r -p "Main Agent Name [$MAIN_AGENT_NAME]: " USER_AGENT </dev/tty
if [[ -n "$USER_AGENT" ]]; then MAIN_AGENT_NAME="$USER_AGENT"; fi

read -r -p "Default Model [$DEFAULT_MODEL]: " USER_MODEL </dev/tty
if [[ -n "$USER_MODEL" ]]; then DEFAULT_MODEL="$USER_MODEL"; fi

read -r -p "Enable Strict Mode (true/false) [$STRICT_MODE]: " USER_STRICT </dev/tty
if [[ -n "$USER_STRICT" ]]; then STRICT_MODE="$USER_STRICT"; fi

# ── Pull the chosen model ──
log "Pulling model '$DEFAULT_MODEL' via Ollama..."
ollama pull "$DEFAULT_MODEL" || echo "WARNING: Failed to pull model '$DEFAULT_MODEL'. You can pull it manually with: ollama pull $DEFAULT_MODEL"

# Call Init
log "Calling /v1/install/init"
if curl -f -X POST http://127.0.0.1:8080/v1/install/init \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"holding_name\":\"$HOLDING_NAME\", \"main_agent_name\":\"$MAIN_AGENT_NAME\", \"default_model\":\"$DEFAULT_MODEL\", \"strict_mode\":$STRICT_MODE, \"vm_provider\":\"incus\"}"; then
  log "Init call successful."
else
  log "Init call failed! Printing backend logs for diagnosis:"
  cd /opt/multiclaw && docker compose -f infra/docker/docker-compose.yml logs multiclawd --tail 200
  sleep 1
fi

echo ""
log "=============================================="
log "  MultiClaw Install Complete!"
log "=============================================="
log "Dashboard URL:  http://${HOST_IP}:3000"
log "API URL:        http://${HOST_IP}:8080/v1"
log "Admin Token:    /var/lib/multiclaw/admin.token"
log "Default Model:  $DEFAULT_MODEL"
log "=============================================="

# ── Verify UI is reachable ──
log "Verifying dashboard is reachable..."
UI_OK=false
for i in {1..15}; do
  if curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/ | grep -q "200"; then
    UI_OK=true
    break
  fi
  sleep 2
done

if $UI_OK; then
  log "Dashboard is UP and reachable at http://${HOST_IP}:3000"
else
  log "WARNING: Dashboard is NOT reachable on port 3000. Printing UI container logs:"
  cd /opt/multiclaw && docker compose -f infra/docker/docker-compose.yml logs ui --tail 50
  echo ""
  log "Container status:"
  docker compose -f infra/docker/docker-compose.yml ps ui
fi
