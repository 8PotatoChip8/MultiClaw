#!/usr/bin/env bash
set -euo pipefail

# MultiClaw PromptFoo Evaluation Runner
#
# One-command script to:
#   1. Reset the holding via API (wipe + reinitialize)
#   2. Wait for agents to boot and org tree to populate
#   3. Run PromptFoo evaluation
#   4. Show results
#
# Usage:
#   ./run.sh              # Full run (setup + eval + results)
#   ./run.sh --skip-setup # Skip setup, run eval on existing holding
#   ./run.sh --setup-only # Only do setup, don't run eval
#   ./run.sh --e2e        # Run E2E tests (setup + full cascade + observe)
#   ./run.sh --e2e --skip-setup  # E2E on existing holding

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

MULTICLAW_URL="${MULTICLAW_URL:-http://localhost:8080}"
SKIP_SETUP=false
SETUP_ONLY=false
E2E=false

for arg in "$@"; do
  case "$arg" in
    --skip-setup) SKIP_SETUP=true ;;
    --setup-only) SETUP_ONLY=true ;;
    --e2e) E2E=true ;;
    --help|-h)
      echo "Usage: $0 [--skip-setup] [--setup-only] [--e2e]"
      echo ""
      echo "  --skip-setup  Run eval against existing holding (no reset)"
      echo "  --setup-only  Only set up a fresh holding, don't run eval"
      echo "  --e2e         Run E2E cascade tests (slow, ~10 min)"
      echo ""
      echo "Environment:"
      echo "  MULTICLAW_URL     Control plane URL (default: http://localhost:8080)"
      exit 0
      ;;
  esac
done

# Check prerequisites
if ! command -v promptfoo &>/dev/null; then
  echo "ERROR: promptfoo not found. Install it with: npm install -g promptfoo"
  exit 1
fi

if ! command -v node &>/dev/null; then
  echo "ERROR: node not found. Install Node.js 18+."
  exit 1
fi

# Check control plane is reachable
echo "Checking control plane at $MULTICLAW_URL..."
if ! curl -sf "$MULTICLAW_URL/v1/health" >/dev/null 2>&1; then
  echo "ERROR: Control plane not reachable at $MULTICLAW_URL"
  echo "Make sure MultiClaw is running: cd /opt/multiclaw/infra/docker && docker compose up -d"
  exit 1
fi
echo "Control plane is healthy."

# Setup
if [ "$SKIP_SETUP" = false ]; then
  echo ""
  echo "════════════════════════════════════════════════════════"
  echo "  Setting up fresh holding for evaluation"
  echo "════════════════════════════════════════════════════════"
  echo ""
  if [ "$E2E" = true ]; then
    # E2E tests create their own companies, so only need MAIN agent ready
    node setup.mjs --quick
  else
    node setup.mjs
  fi

  if [ "$SETUP_ONLY" = true ]; then
    echo "Setup complete. Exiting (--setup-only)."
    exit 0
  fi
fi

# Create results directory (may need sudo if /opt/multiclaw is root-owned)
if ! mkdir -p results 2>/dev/null; then
  echo "Cannot create results/ directory (permission denied)."
  echo "Fix with: sudo chmod -R a+rw /opt/multiclaw/tests/promptfoo"
  exit 1
fi

# Run PromptFoo evaluation
echo ""
echo "════════════════════════════════════════════════════════"
echo "  Running PromptFoo evaluation"
echo "════════════════════════════════════════════════════════"
echo ""
if [ "$E2E" = true ]; then
  CONFIG_FILE="e2e-promptfooconfig.yaml"
  RESULTS_FILE="results/e2e-results.json"
else
  CONFIG_FILE="promptfooconfig.yaml"
  RESULTS_FILE="results/eval-results.json"
fi

promptfoo eval --config "$CONFIG_FILE"

# Show results summary
echo ""
echo "════════════════════════════════════════════════════════"
echo "  Results"
echo "════════════════════════════════════════════════════════"
echo ""
echo "Results saved to: $RESULTS_FILE"
echo ""
echo "View interactive results:"
echo "  promptfoo view"
echo ""
