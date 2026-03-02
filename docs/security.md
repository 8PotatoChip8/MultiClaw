# Security Architecture

## Tokens and Encrypted At Rest
Every API token, VM proxy token, and OpenClaw gateway access token is encrypted at rest using AES-GCM wrapping keys in postgres. MultiClaw derives its master encryption key from `/var/lib/multiclaw/master.key` (0600 root).

## Networking
- The OpenClaw API is bound to loopback `127.0.0.1:18789` on each VM.
- `multiclaw-agentd` initiates outbound connections to `multiclawd` only.
- The `ollama-proxy` binds to `0.0.0.0:11436` but drops packets not from the Incus subnet.

## Panic Operations
The Quick-Panic button stops a VM forcefully and revokes its API token bindings, immediately nullifying pending actions.
