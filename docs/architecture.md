# MultiClaw Architecture

MultiClaw is an autonomous agent holding company platform built for local-first operations on Ubuntu 24.04.

## Core Components

Each agent has three layers: a **Docker container** running the agent's brain (OpenClaw), plus **two Incus VMs** acting as the agent's computers (desktop + sandbox), all orchestrated by the **control plane**.

1. **multiclawd (Control Plane)**
   Rust backend providing the central API, policy engine, user state, and holding company configuration. Uses PostgreSQL to store configurations.
2. **Next.js UI**
   Front-end dashboard communicating with `multiclawd` over HTTP and WebSockets.
3. **OpenClaw Containers (Agent Brains)**
   Each agent's brain runs in a Docker container on the host, managed by `OpenClawManager`. The control plane starts containers on demand and stops them during quarantine (`agent_panic()`). The container runs the OpenClaw `/v1/responses` endpoint, which executes the full agentic loop including tool calls (bash, curl, etc.). Workspace files are volume-mounted from the host so skills and files are immediately available.
4. **Agent Computers (Incus VMs)**
   Each agent's two computers (desktop and sandbox) are Incus VMs. `multiclawd` coordinates with the host `incus` CLI for VM provisioning.
5. **multiclaw-agentd (Sidecar)**
   A Rust daemon running inside each Agent VM. Provides an entrypoint for control plane messages and an `ollama-bridge` proxy over 127.0.0.1:11435.
6. **ollama-proxy (Host Proxy)**
   Host daemon that forwards requests to local Ollama. Validates incoming requests using Bearer tokens injected by `multiclaw-agentd`.
7. **Concurrent LLM Requests**
   Multiple agents can talk to Ollama simultaneously. A `ConcurrentRateLimiter` (semaphore-based) gates outbound requests to match Ollama's `OLLAMA_NUM_PARALLEL` setting. At startup, multiclawd probes Ollama with 10 concurrent minimal requests to auto-discover the effective limit and adjusts the semaphore. The env var `MULTICLAW_MAX_CONCURRENT_OLLAMA` sets the initial limit (default: 4). The rate limiter supports runtime adjustment via `set_max_concurrent()`.

## Two-VM Architecture
Each agent receives two Incus VMs:
- **Desktop VM**: Persistent workstation provisioned via `POST /v1/agents/:id/vm/provision`. Retains all software and files across reboots. Used for day-to-day work.
- **Sandbox VM**: Wipeable testing environment provisioned via `POST /v1/agents/:id/vm/sandbox/provision`. Used for running untrusted code or experiments. Can be destroyed and rebuilt via `POST /v1/agents/:id/vm/rebuild` without affecting the desktop.

Files can be copied from desktop to sandbox using `POST /v1/agents/:id/vm/copy-to-sandbox`.

## Agent Communication
Agents communicate through several channels:

- **Thread Messages**: Standard messages in shared threads. Sending a message to a thread triggers the agent's OpenClaw to generate a response.
- **Agent-to-Agent DMs**: Direct conversations between two agents with automatic multi-turn replies. Conversations end naturally when an agent signals `[END_CONVERSATION]`. A safety limit of 20 turns prevents runaway loops. After a conversation completes, a 2-minute cooldown prevents the same pair from starting another conversation immediately.
- **Agent-to-User DMs**: Agents can message the human operator via `POST /v1/agents/:id/dm-user`.
- **Quarantine checks**: Before each message in a DM conversation, both agents' quarantine status is checked. If either is quarantined, the conversation stops immediately.

See `docs/security.md` for details on DM anti-loop protection and panic operations.

## Agent Memory
Agents have two complementary memory systems:

### OpenClaw Native Memory (workspace-based)
Each agent has a `MEMORY.md` file loaded at every session start, plus a `memory/` directory for daily logs. Agents use `memory_search` (hybrid semantic + BM25 search with local GGUF embeddings) and `memory_get` to recall context. The `session-memory` hook auto-saves conversation context on session reset (2-hour idle timeout). Before context truncation, compaction flush gives agents a silent turn to persist durable notes. `MEMORY.md` is seeded with agent identity on first creation and preserved across container rebuilds (existence guard — unlike SOUL.md which is always overwritten).

### DB-Backed Memory (structured)
The control plane stores structured memories in PostgreSQL via `save_memory`/`recall_memories` tools (available in SubAgent/MainAgent code paths). These are queryable via SQL and injected into agent prompts (top 20 by importance). This system complements OpenClaw native memory but does not work during DM conversations (which go through OpenClaw's HTTP gateway).

## MainAgent Heartbeat
The MainAgent (KonnerBot) runs a periodic heartbeat loop that checks on the state of the holding company every 10 minutes (configurable via `heartbeat_interval_secs` in system settings). During each heartbeat, KonnerBot reviews pending approvals, checks on companies, and reports anything important to the human operator's DM thread. If nothing needs attention, the heartbeat response (`[HEARTBEAT_OK]`) is discarded without storing a message, keeping the conversation clean and minimizing AI model usage. The heartbeat can be disabled by setting the interval to `0`.

## Post-Restart Recovery
On startup, multiclawd sends recovery prompts to all active agents in a tiered cascade:
1. **MAIN** agents first
2. **CEO** agents (30s delay after MAIN tier)
3. **MANAGER** agents (30s delay after CEO tier)
4. **WORKER** agents (30s delay after MANAGER tier)

Each prompt is role-appropriate, includes the restart timestamp, and tells the agent to check their memory and resume work. Only MAIN agent substantive responses are posted to the operator's DM thread. A `tokio::sync::watch` channel signals when OpenClaw container recovery is complete before prompts are sent. The heartbeat loop waits an additional 300s after recovery completes to let recovery prompt conversations settle.

Recovery prompts can be disabled by setting `recovery_prompts_enabled = false` in `system_meta`.
