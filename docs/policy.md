# Agent Tool Policies

## Overlap with OpenClaw
OpenClaw provides extensive tools and system access. MultiClaw limits this via Tool Policies sent in `openclaw.json`.

## Default Role Policies
- **CEO Policy**: Can use browser, file workspace editing, and coding tools. Host-level tools are denied.
- **Manager Policy**: Can use browser, file workspace editing. Cannot run untrusted binaries (`system.run` denied by default).
- **Worker Policy**: Can use browser and file workspace tools on pre-defined workspaces. `system.run` denied by default.

## Strict Mode
MultiClaw can be initialized in "Strict Mode", which configures tighter OpenClaw constraints and requires User approvals for cross-VM networking or running commands.
