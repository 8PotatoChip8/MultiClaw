# MultiClaw

A local-first "agent holding company" platform for Ubuntu Desktop 24.x.

## Overview
MultiClaw orchestrates full-fledged autonomous agents inside isolated Incus VMs using OpenClaw, communicating with a host-local Ollama and a Rust-based control plane. A Next.js UI serves as the unified dashboard for the holding company.

## Quick Start
```bash
curl -fsSL https://raw.githubusercontent.com/8PotatoChip8/MultiClaw/main/infra/install/install.sh | bash
```

## Architecture

- **multiclawd (Control Plane)**: Rust backend containing the rules engine and agent supervision logic.
- **ollama-proxy**: Extends local Ollama with Bearer Token auth to gate API access per VM.
- **Next.js Dashboard**: Canonical UI.
- **Agent VMs**: Each agent runs in their own Incus VM with an `openclaw-gateway` bound to loopback and a sidecar `multiclaw-agentd`.

See `docs/architecture.md` for more details.

## Adding Models
To add models, use the `multiclaw` CLI or the Dashboard.
Under the hood, this pulls the model to the host Ollama context and adjusts the agent's allowlist.

## Security Warning
This software enables autonomous agents to execute code and use web browsers. Running agents in strict mode and preserving approvals boundaries for structural changes is strongly recommended.

## Troubleshooting
See `docs/runbook.md` for daily operational checklists.
