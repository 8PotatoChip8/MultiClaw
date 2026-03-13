# VM Providers

## Incus
MultiClaw uses Incus VMs provisioned via Rust subprocess calls from `multiclawd`. A default network `multiclawbr0` is created during installation.

Each agent receives **two** Incus VMs:
- **Desktop VM**: Persistent workstation for day-to-day work. Retains all installed software and files across reboots.
- **Sandbox VM**: Wipeable testing environment for running untrusted code or experiments. Can be destroyed and rebuilt at any time without affecting the desktop.

### Naming Convention
VM names use the first 8 characters of the agent's UUID:
- Desktop: `mc-{uuid-prefix}` (e.g., `mc-3a96c961`)
- Sandbox: `mc-{uuid-prefix}-sb` (e.g., `mc-3a96c961-sb`)

### VM User
VMs are provisioned with an `employee` user at UID 1000 via cloud-init. All commands default to running as this user.

### Token Rotation
If an Agent VM needs to be refreshed:
```bash
# Destroys the sandbox VM and recreates it. Desktop VMs cannot be rebuilt.
multiclaw agents id <AGENT_ID> vm rebuild
```
