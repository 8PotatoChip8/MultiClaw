# VM Providers

## Incus (MVP)
The MVP uses Incus containers/VMs provisioned directly via `subprocess` from `multiclawd`'s Node interface.
A default network `multiclawbr0` is created.
Each Agent maps exactly 1-to-1 to an Incus VM.

### Naming Convention
Names are structured as `mc-<companySlug>-<role>-<n>`:
- `mc-mainhc-ceo-1`
- `mc-acmecorp-mgr-2`
- `mc-acmecorp-wkr-1`

### Token Rotation
If an Agent VM needs to be refreshed:
```bash
# Deletes the VM and recreates it using the same DB agent_id, pulling new tokens.
multiclaw agents id <AGENT_ID> vm rebuild
```
