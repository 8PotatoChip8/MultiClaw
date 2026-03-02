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
