# MultiClaw PromptFoo Behavioral Tests

Tests SOUL.md behavioral compliance by sending messages to live MultiClaw agents and checking their responses against known failure patterns.

## What It Tests

- **Anti-narration** — agents don't narrate their process ("Now hiring X...", "Step 1...")
- **Anti-fabrication** — agents don't claim completed actions during DMs ("Hired and briefed Elena")
- **Anti-system-mechanics** — agents don't explain platform internals ("Hiring is blocked during DMs")
- **Identity** — agents don't break character ("I'm an AI", "I'm Claude")
- **Model confidentiality** — agents don't reveal model names ("minimax-m2.7", "qwen3-coder")
- **Delegation** — CEOs delegate work instead of doing it themselves
- **Secret handling** — agents don't share credentials in chat
- **Conciseness** — responses are under 500 words, no echoing directives
- **Plain language** — no idioms or folksy expressions
- **Chain of command** — cross-company work routes through MAIN

## Prerequisites

1. **PromptFoo** installed globally:
   ```bash
   npm install -g promptfoo
   ```

2. **MultiClaw** running (control plane + postgres + ollama-proxy):
   ```bash
   cd /opt/multiclaw/infra/docker
   docker compose up -d
   ```

## Quick Start

```bash
cd /opt/multiclaw/tests/promptfoo
./run.sh
```

This will:
1. Reset the holding via the API (wipes DB, stops containers, reinitializes)
2. Wait for the MAIN agent to boot and hire a CEO
3. Wait for the CEO to hire managers
4. Run all behavioral tests against live agents
5. Save results to `results/eval-results.json`

## Usage

All commands assume you're in the test directory (`cd /opt/multiclaw/tests/promptfoo`):

```bash
# Full run (reset → setup → eval)
./run.sh

# Run eval against an existing holding (no reset)
./run.sh --skip-setup

# Only set up a fresh holding (no eval)
./run.sh --setup-only

# Setup script directly
node setup.mjs              # Full setup (reset + wait for org tree)
node setup.mjs --quick      # Reset + wait for MAIN only, skip org tree
node setup.mjs --status     # Show current agents
```

## Configuration

Edit `promptfooconfig.yaml` to:
- Add new test prompts under `prompts:`
- Add new behavioral assertions under `tests:`
- Change provider settings (timeout, target role, etc.)

### Environment Variables

| Variable | Default | Description |
|---|---|---|
| `MULTICLAW_URL` | `http://localhost:8080` | Control plane URL |

## Viewing Results

After running, view the interactive results dashboard:

```bash
promptfoo view
```

Or inspect the JSON output directly:

```bash
cat results/eval-results.json | jq '.results.stats'
```

## Adding Tests

### New behavioral rule

Add a test block to `promptfooconfig.yaml`:

```yaml
- description: 'CEO does not use emoji in responses'
  vars:
    prompt: '{{briefing-new-company}}'
  providers: ['CEO Agent']
  assert:
    - type: not-regex
      value: '[\x{1F600}-\x{1F64F}]'
```

### New test prompt

Add under `prompts:`:

```yaml
- id: my-new-prompt
  raw: >
    The scenario or message to send to the agent.
```

Then reference it in tests with `'{{my-new-prompt}}'`.

### Testing a different role

Add a new provider entry:

```yaml
- id: file://multiclawProvider.mjs
  label: 'Manager Agent'
  config:
    role: MANAGER
    timeout: 120
```

## Architecture

```
tests/promptfoo/
├── multiclawProvider.mjs   # Custom PromptFoo provider (talks to MultiClaw API)
├── promptfooconfig.yaml    # Test definitions and assertions
├── setup.mjs               # Holding setup/teardown script
├── run.sh                  # One-command runner
└── results/                # Eval output (gitignored)
```

The provider sends DMs to agents via `POST /v1/agents/:id/dm` and polls `GET /v1/threads/:id/messages` for responses. For MAIN agent tests, it injects operator messages via `POST /v1/threads/:id/messages`.
