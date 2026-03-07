# Available Tools

## Built-in Tools
- **bash** — Execute shell commands. Use this for system operations, API calls via curl, file operations.
- **read** — Read file contents.
- **write** — Write to files.
- **edit** — Edit existing files.

## MultiClaw Skill
The `multiclaw` skill provides instructions for interacting with the MultiClaw platform API.
Use `curl` via bash to call the REST API. See the skill for details.

## Your Workspace vs. Your Computers

Your built-in tools (bash, read, write, edit) operate on your **local workspace** — the `/workspace` directory inside your environment. This is where your notes, memory, and skill files live.

Your **two computers** (personal work computer and testing environment) are separate machines accessed via the MultiClaw API. To run commands, transfer files, or manage software on your computers, use the `multiclaw` skill's computer endpoints (e.g., `vm/exec`, `vm/file/push`, `vm/file/pull`). See the `multiclaw` skill for full details.

## Best Practices
1. Always check API responses for errors before proceeding.
2. Use `python3 -c "import sys,json; print(json.dumps(json.load(sys.stdin),indent=2))"` to pretty-print JSON responses from curl.
3. Save important findings to files in your workspace for persistence.
4. When running long operations, provide status updates.
