#!/usr/bin/env bash
set -euo pipefail

# MultiClaw PromptFoo Evaluation Runner
#
# One-command script to:
#   1. Teardown any existing holding
#   2. Initialize a fresh holding
#   3. Wait for agents to boot and org tree to populate
#   4. Run PromptFoo evaluation
#   5. Show results
#
# Usage:
#   ./run.sh              # Full run (setup + eval + results)
#   ./run.sh --skip-setup # Skip setup, run eval on existing holding
#   ./run.sh --setup-only # Only do setup, don't run eval

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

MULTICLAW_URL="${MULTICLAW_URL:-http://localhost:8080}"
SKIP_SETUP=false
SETUP_ONLY=false

for arg in "$@"; do
  case "$arg" in
    --skip-setup) SKIP_SETUP=true ;;
    --setup-only) SETUP_ONLY=true ;;
    --help|-h)
      echo "Usage: $0 [--skip-setup] [--setup-only]"
      echo ""
      echo "  --skip-setup  Run eval against existing holding (no teardown/init)"
      echo "  --setup-only  Only set up a fresh holding, don't run eval"
      echo ""
      echo "Environment:"
      echo "  MULTICLAW_URL     Control plane URL (default: http://localhost:8080)"
      echo "  MULTICLAW_DB_URL  Postgres URL (default: postgresql://multiclaw:multiclaw_pass@localhost:5432/multiclaw)"
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
  echo "Make sure MultiClaw is running: cd infra/docker && docker compose up -d"
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
  node setup.mjs

  if [ "$SETUP_ONLY" = true ]; then
    echo "Setup complete. Exiting (--setup-only)."
    exit 0
  fi
fi

# Create results directory
mkdir -p results

# Run PromptFoo evaluation
echo ""
echo "════════════════════════════════════════════════════════"
echo "  Running PromptFoo evaluation"
echo "════════════════════════════════════════════════════════"
echo ""
promptfoo eval --config promptfooconfig.yaml

# Show results summary
echo ""
echo "════════════════════════════════════════════════════════"
echo "  Results"
echo "════════════════════════════════════════════════════════"
echo ""
echo "Results saved to: results/eval-results.json"
echo ""
echo "View interactive results:"
echo "  promptfoo view"
echo ""
