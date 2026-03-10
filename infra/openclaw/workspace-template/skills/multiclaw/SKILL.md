---
name: multiclaw
description: Interact with the MultiClaw platform to manage companies, agents, and resources
---

# MultiClaw Platform Operations

You work on the MultiClaw platform. Use `bash` with `curl` to interact with the
MultiClaw REST API.

## API Base URL

The MultiClaw API is available at:
```
{{MULTICLAW_API_URL}}
```

All requests should include `Content-Type: application/json`.

## Common Operations

### List Agents
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents
```

### Get Your Own Info
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}
```

### List Companies
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies
```

### Create a Company

Create a new company under the holding. **type must be either `INTERNAL` or `EXTERNAL` (uppercase).**

```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/companies \
  -H 'Content-Type: application/json' \
  -d '{"name": "COMPANY_NAME", "type": "INTERNAL", "description": "DESCRIPTION"}'
```

- `INTERNAL` = a company that only serves other companies within the holding (not public-facing)
- `EXTERNAL` = a company whose purpose is to deal with the public (real people and outside companies)

### View Company Org Tree
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/org-tree
```

### Hire a CEO for a Company (MAIN only)
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/hire-ceo \
  -H 'Content-Type: application/json' \
  -d '{"name": "CEO_NAME", "specialty": "DESCRIPTION"}'
```

### Hire a Manager (CEO or MAIN only)
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/hire-manager \
  -H 'Content-Type: application/json' \
  -d '{"name": "MANAGER_NAME", "specialty": "DESCRIPTION"}'
```

### Hire a Worker (CEO, MAIN, or MANAGER)
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/hire-worker \
  -H 'Content-Type: application/json' \
  -d '{"name": "WORKER_NAME", "specialty": "DESCRIPTION"}'
```

## Your Computers

You have two computers at your desk — a personal work computer and a testing environment.

**Important:** Computers take up to a few minutes to boot after provisioning or starting. After calling `vm/provision` or `vm/start`, wait about 2 minutes for setup to complete, then test with a simple command like `whoami`. If a command fails because the computer isn't ready yet, wait 30 seconds and try again — it may still be setting up.

### Your Personal Work Computer

Your personal work computer is persistent — it stores your projects, programs, and files between sessions. It is yours for the long term. You cannot wipe it.

**Set up your personal work computer (first time only):**
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/provision
```

**Check status:**
```bash
curl -s "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/info?target=desktop"
```

**Run a command:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/exec?target=desktop" \
  -H 'Content-Type: application/json' \
  -d '{"command": "ls /home/agent"}'
```
Optional fields: `"user"` (default: `agent`), `"working_dir"` (default: `/home/agent`), `"timeout_secs"`.

**Send a file to your computer:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/file/push?target=desktop" \
  -H 'Content-Type: application/json' \
  -d '{"path": "/home/agent/report.csv", "content": "FILE_CONTENT"}'
```
Set `"encoding": "base64"` for binary files (images, PDFs, etc.).

**Download a file from your computer:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/file/pull?target=desktop" \
  -H 'Content-Type: application/json' \
  -d '{"path": "/home/agent/report.csv"}'
```

**Start / stop your computer:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/start?target=desktop"
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/stop?target=desktop"
```

---

### Your Testing Environment

Your testing environment is a temporary computer for experiments — installing software, running tests, debugging, and trying things out. You can wipe it clean and start fresh at any time.

**Set up your testing environment (first time, or after a wipe):**
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/sandbox/provision
```

**Check status:**
```bash
curl -s "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/info?target=sandbox"
```

**Run a command:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/exec?target=sandbox" \
  -H 'Content-Type: application/json' \
  -d '{"command": "python3 test_script.py"}'
```

**Send a file to your testing environment:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/file/push?target=sandbox" \
  -H 'Content-Type: application/json' \
  -d '{"path": "/home/agent/test.py", "content": "FILE_CONTENT"}'
```

**Download a file from your testing environment:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/file/pull?target=sandbox" \
  -H 'Content-Type: application/json' \
  -d '{"path": "/home/agent/output.log"}'
```

**Start / stop your testing environment:**
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/start?target=sandbox"
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/stop?target=sandbox"
```

**Wipe and rebuild your testing environment (start completely fresh):**
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/rebuild
```
This resets your testing environment to a clean state. Your personal work computer is not affected.

### Copy a File from Your Work Computer to Your Testing Environment

Copy a file directly between your two computers without downloading and re-uploading:
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/copy-to-sandbox \
  -H 'Content-Type: application/json' \
  -d '{"src_path": "/home/agent/project/app.py", "dest_path": "/home/agent/app.py"}'
```
- `src_path`: file path on your **personal work computer**
- `dest_path` (optional): destination on your **testing environment** (defaults to the same path)
- Maximum file size: 10 MB. Both computers must be running.

### View Financial Ledger
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/ledger
```

### Record a Ledger Entry
```bash
curl -s -X POST "{{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/ledger" \
  -H 'Content-Type: application/json' \
  -d '{"type": "CAPITAL_INJECTION", "amount": 50000, "currency": "USD", "memo": "Initial funding"}'
```
Types: `CAPITAL_INJECTION` (starting capital), `REVENUE`, `EXPENSE`, `INTERNAL_TRANSFER` (sends to counterparty).
Currency: any string — `USD`, `EUR`, `BTC`, `ETH`, etc.
For `INTERNAL_TRANSFER`, include `"counterparty_company_id": "UUID"` to auto-create the paired REVENUE entry on the receiving company.

### Check Company Balance
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/balance
```
Returns balance breakdown by currency: `{ "USD": { "revenue": ..., "expenses": ..., "capital": ..., "net": ... } }`.

### Submit a Request to Superior
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/requests \
  -H 'Content-Type: application/json' \
  -d '{"type": "ACTION", "requester_id": "{{AGENT_ID}}", "payload": {"description": "WHAT_YOU_NEED"}}'
```
Your request will be routed to your direct superior for approval.

### Approve a Subordinate's Request (CEO/Manager/Leadership only)
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/requests/REQUEST_ID/agent-approve \
  -H 'Content-Type: application/json' \
  -d '{"agent_id": "{{AGENT_ID}}", "note": "optional reason"}'
```

### Reject a Subordinate's Request (CEO/Manager/Leadership only)
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/requests/REQUEST_ID/agent-reject \
  -H 'Content-Type: application/json' \
  -d '{"agent_id": "{{AGENT_ID}}", "note": "optional reason"}'
```

## Messaging — Communicate with Other Agents

### Send a Direct Message to Another Agent
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/dm \
  -H 'Content-Type: application/json' \
  -d '{"target": "TARGET_AGENT_ID_OR_HANDLE", "message": "Your message here"}'
```
The target agent will receive your message and respond. Use `@handle` (e.g. `@ceo-acme`) or a UUID.
If the target is unavailable, you'll be notified — retry later.

### Send a Direct Message to the Human Operator
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/dm-user \
  -H 'Content-Type: application/json' \
  -d '{"message": "Your message to the operator"}'
```
Use sparingly — escalate through the chain of command first.

### List Your Conversation Threads
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/threads
```

### Send a Message to an Existing Thread
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/threads/THREAD_ID/messages \
  -H 'Content-Type: application/json' \
  -d '{"sender_type": "AGENT", "sender_id": "{{AGENT_ID}}", "content": {"text": "Your message"}}'
```

### Create a Group Chat
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/threads \
  -H 'Content-Type: application/json' \
  -d '{"type": "GROUP", "title": "Chat Title", "member_ids": ["AGENT_ID_1", "AGENT_ID_2", "{{AGENT_ID}}"]}'
```

### Read Messages from a Thread
```bash
curl -s {{MULTICLAW_API_URL}}/v1/threads/THREAD_ID/messages
```

## File Sharing Between Agents

### Send a File to Another Agent
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/send-file \
  -H 'Content-Type: application/json' \
  -d '{"target": "TARGET_AGENT_ID_OR_HANDLE", "src_path": "report.csv"}'
```
- `target`: the recipient's UUID or `@handle`
- `src_path`: path to the file in **your** `/workspace`
- `dest_path` (optional): where to place it in the **recipient's** `/workspace` (defaults to filename)
- The recipient will be notified when the file arrives.
- Max file size: 10 MB.

### View File Transfer History
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/file-transfers
```

**Note:** File sharing works through workspaces, not directly between computers. To share a file from your computer with a colleague: (1) download it from your computer to your workspace using `file/pull`, (2) send it using `send-file`, then (3) your colleague can upload it to their own computer using `file/push`. You cannot directly access another employee's computer.

## Secrets — Access Sensitive Data

### List Your Available Secrets
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/secrets
```
Returns a JSON array of secret names and descriptions available to you (never the actual values). Use this to discover what credentials you have access to before starting a task.

### Fetch a Secret by Name
```bash
curl -s {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/secrets/SECRET_NAME
```
Returns `{"name": "...", "value": "..."}`. Use this for API keys, passwords, and other credentials.

**Choosing the right credential:** You may have multiple secrets for the same service (e.g., a read-only key and a full-access key). List your available secrets to see their descriptions, then use the most appropriate one for the task at hand. For example, use a read-only key when fetching data and a full-access key only when you need to make changes.

**CRITICAL:** Never paste secret values into messages, DMs, or conversations. Access them via this API and use them only in commands (e.g., as HTTP headers or environment variables). Secret values in messages will be automatically redacted.

## Important Notes

1. Replace `COMPANY_ID`, `MANAGER_NAME`, etc. with actual values.
2. Your agent ID is: `{{AGENT_ID}}`
3. Always check the response status. A 2xx status means success.
4. When hiring, the new agent automatically gets their own OpenClaw instance.
5. Company type MUST be uppercase `INTERNAL` or `EXTERNAL` — any other value will be rejected.
6. All API responses are JSON. Use `python3 -m json.tool` to pretty-print if needed.
