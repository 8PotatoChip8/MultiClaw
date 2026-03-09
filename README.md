# MultiClaw

A local-first "agent holding company" platform for Ubuntu Desktop 24.x.

## Overview
MultiClaw orchestrates autonomous AI agents as Docker containers (via OpenClaw), each with access to isolated Incus VMs for code execution, all communicating through a host-local Ollama and a Rust-based control plane. A Next.js UI serves as the unified dashboard for the holding company.

## Quick Start

### Stable Release (recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/8PotatoChip8/MultiClaw/main/infra/install/install-stable.sh | sudo bash
```

### Development Build
```bash
curl -fsSL https://raw.githubusercontent.com/8PotatoChip8/MultiClaw/main/infra/install/install.sh | sudo bash
```

## Architecture

- **multiclawd (Control Plane)**: Rust backend containing the rules engine and agent supervision logic.
- **OpenClaw Containers**: Each agent's brain runs in a Docker container on the host, managed by `OpenClawManager`. Containers execute the agentic loop (LLM reasoning + tool calls).
- **Agent Computers (Incus VMs)**: Each agent gets two Incus VMs — a persistent desktop and a wipeable sandbox — for running code, installing software, and experimenting.
- **ollama-proxy**: Extends local Ollama with Bearer Token auth to gate API access per agent.
- **Next.js Dashboard**: Canonical UI.

See `docs/architecture.md` for more details.

## Update Channels (Auto-Updater)
MultiClaw supports a 3-channel auto-updater customizable in the Settings page:
- **Stable**: Fully tested production releases checking GitHub `releases/latest`.
- **Beta**: Experimental features checking the `beta` branch.
- **Dev**: Bleeding-edge features checking the `main` branch.

## Agent Roles & Hierarchies
Each agent runs with role-specific "brain files" (SOUL.md, AGENTS.md, SKILL.md) to define their permissions:
1. **MAIN (MainAgent)**: Top-level holding agent. Can create companies and hire CEOs.
2. **CEO**: Runs a company. Can hire managers and workers. Cannot create companies.
3. **MANAGER**: Manages a department. Can hire workers only.
4. **WORKER**: Executes tasks. Has no hiring or management authority.

## Agent Computers
Each agent receives two Incus VMs:
- **Desktop VM**: A persistent workstation for day-to-day work. Survives reboots and retains all installed software and files.
- **Sandbox VM**: A wipeable testing environment for running untrusted code or experiments. Can be rebuilt at any time without affecting the desktop.

Agents can copy files between their desktop and sandbox via the control plane. See `docs/architecture.md` for details.

## Inter-Agent Communication
Agents communicate through direct messages (DMs) and group threads. DMs support automatic multi-turn conversations with depth limits and cooldown protections to prevent runaway loops. The **Agent Comms** page in the dashboard shows all agent conversations.

Agents can also DM the human operator directly. See `docs/security.md` for DM safety details.

## Secrets Management
Store API keys, credentials, and other secrets for your agents using the Secrets API. Secrets can be scoped to an individual agent, an entire company, or the whole holding. Values are encrypted at rest with AES-GCM and automatically scrubbed from all stored messages.

Agents retrieve secrets by name with hierarchical lookup (agent → company → holding). See `docs/security.md` for full details.

## Adding Models
To add models, use the `multiclaw` CLI or the Dashboard.
Under the hood, this pulls the model to the host Ollama context and adjusts the agent's allowlist.

## Security Warning
This software enables autonomous agents to execute code and use web browsers. Running agents in strict mode and preserving approvals boundaries for structural changes is strongly recommended.

## Troubleshooting
See `docs/runbook.md` for daily operational checklists.
