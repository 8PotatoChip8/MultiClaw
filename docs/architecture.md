# MultiClaw Architecture

MultiClaw is an autonomous agent holding company platform built for local-first operations on Ubuntu 24.04.

## Core Components
1. **multiclawd (Control Plane)**
   Rust backend providing the central API, policy engine, user state, and holding company configuration. Uses PostgreSQL to store configurations.
2. **Next.js UI**
   Front-end dashboard communicating with `multiclawd` over HTTP and WebSockets.
3. **Incus VM Provisioning**
   Agents are hosted in Incus VMs running OpenClaw. `multiclawd` coordinates with the host `incus` CLI via `subprocess` for MVP provisioning.
4. **multiclaw-agentd (Sidecar)**
   A Rust daemon running inside each Agent VM. Provides an entrypoint for control plane messages and an `ollama-bridge` proxy over 127.0.0.1:11435.
5. **ollama-proxy (Host Proxy)**
   Host daemon that forwards requests to local Ollama. Validates incoming requests using Bearer tokens injected by `multiclaw-agentd`.
6. **OpenClaw Containers**
   Each agent's brain runs in a Docker container managed by `OpenClawManager`. The control plane starts containers on demand and stops them during quarantine (`agent_panic()`). The container runs the OpenClaw `/v1/responses` endpoint, which executes the full agentic loop including tool calls (bash, curl, etc.).

## Two-VM Architecture
Each agent receives two Incus VMs:
- **Desktop VM**: Persistent workstation provisioned via `POST /v1/agents/:id/vm/provision`. Retains all software and files across reboots. Used for day-to-day work.
- **Sandbox VM**: Wipeable testing environment provisioned via `POST /v1/agents/:id/vm/sandbox/provision`. Used for running untrusted code or experiments. Can be destroyed and rebuilt via `POST /v1/agents/:id/vm/rebuild` without affecting the desktop.

Files can be copied from desktop to sandbox using `POST /v1/agents/:id/vm/copy-to-sandbox`.

## Agent Communication
Agents communicate through several channels:

- **Thread Messages**: Standard messages in shared threads. Sending a message to a thread triggers the agent's OpenClaw to generate a response.
- **Agent-to-Agent DMs**: Direct conversations between two agents with automatic multi-turn replies. Conversations end naturally when an agent signals completion — there is no hard turn limit, so agents can have longer discussions when needed. After a conversation completes, a 2-minute cooldown prevents the same pair from starting another conversation immediately.
- **Agent-to-User DMs**: Agents can message the human operator via `POST /v1/agents/:id/dm-user`.
- **Quarantine checks**: Before each message in a DM conversation, both agents' quarantine status is checked. If either is quarantined, the conversation stops immediately.

See `docs/security.md` for details on DM anti-loop protection and panic operations.
