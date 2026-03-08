# MultiClaw API Reference (multiclawd)

## Base URL
Default local binding: `http://127.0.0.1:8080/v1/`

## Authorization
All endpoints (except `/v1/install/init` and `/v1/health`) require a Bearer token generated at install:
```
Authorization: Bearer <token>
```

---

## Health & Installation

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/health` | Check API and database health |
| POST | `/v1/install/init` | Initialize system with holding company and MainAgent |

---

## Companies

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/companies` | List all companies |
| POST | `/v1/companies` | Create a new company |
| GET | `/v1/companies/:id` | Get company details |
| PATCH | `/v1/companies/:id` | Update company (name, type, description, status) |
| GET | `/v1/companies/:id/org-tree` | Get organizational hierarchy |
| POST | `/v1/companies/:id/hire-ceo` | Start CEO hiring workflow |
| GET | `/v1/companies/:id/ledger` | Get company financial ledger |

**Hire CEO body:**
```json
{
  "name": "Agent Name",
  "specialty": "domain expertise",
  "preferred_model": "model-name"  // optional, overrides default
}
```

---

## Agents

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/agents` | List all agents |
| GET | `/v1/agents/:id` | Get agent details |
| PATCH | `/v1/agents/:id` | Update agent (preferred_model, specialty, system_prompt) |
| POST | `/v1/agents/:id/hire-manager` | Hire a manager (policy-checked) |
| POST | `/v1/agents/:id/hire-worker` | Hire a worker (policy-checked) |
| POST | `/v1/agents/:id/panic` | Quarantine agent — stops container, blocks all DMs |
| GET | `/v1/agents/:id/thread` | Get or create a DM thread with this agent |
| POST | `/v1/agents/:id/dm` | Send agent-to-agent DM (auto-conversation loop) |
| POST | `/v1/agents/:id/dm-user` | Agent sends a message to the human operator |
| POST | `/v1/agents/:id/send-file` | Send a file to another agent (policy-checked, max 10 MB) |
| GET | `/v1/agents/:id/file-transfers` | List file transfers involving this agent |
| GET | `/v1/agents/:id/threads` | Get all threads this agent participates in |
| GET | `/v1/agents/:id/memories` | List agent memories/knowledge base |
| POST | `/v1/agents/:id/memories` | Create or update an agent memory |
| DELETE | `/v1/agents/:id/memories/:mid` | Delete a memory entry |
| GET | `/v1/agents/:id/openclaw-files` | Read agent's OpenClaw workspace files |
| GET | `/v1/agents/:id/secrets/:name` | Fetch a secret by name (hierarchical lookup) |

**Agent-to-Agent DM body:**
```json
{
  "from_agent_id": "uuid",
  "message": "text content"
}
```
DM conversations auto-loop until agents naturally conclude the discussion. A safety ceiling of 50 turns prevents runaway loops, and a 2-minute cooldown between the same pair prevents re-initiation. Both agents' quarantine status is checked before each message.

**Send File body:**
```json
{
  "to_agent_id": "uuid",
  "file_path": "/path/on/sender/vm",
  "description": "what this file is"
}
```

**Hire Manager/Worker body:**
```json
{
  "name": "Agent Name",
  "specialty": "domain expertise",
  "preferred_model": "model-name"  // optional
}
```

---

## VM Management

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/agents/:id/vm/provision` | Provision a persistent desktop VM |
| POST | `/v1/agents/:id/vm/sandbox/provision` | Provision a wipeable sandbox VM |
| POST | `/v1/agents/:id/vm/start` | Start a VM (`?target=desktop` or `?target=sandbox`) |
| POST | `/v1/agents/:id/vm/stop` | Stop a VM |
| POST | `/v1/agents/:id/vm/rebuild` | Destroy and rebuild sandbox VM (desktop cannot be rebuilt) |
| POST | `/v1/agents/:id/vm/exec` | Execute a command on the VM |
| GET | `/v1/agents/:id/vm/info` | Get VM state, IP address, and resources |
| POST | `/v1/agents/:id/vm/file/push` | Write a file to the VM (base64 or text) |
| POST | `/v1/agents/:id/vm/file/pull` | Read a file from the VM |
| POST | `/v1/agents/:id/vm/copy-to-sandbox` | Copy a file from the desktop VM to the sandbox VM |

**VM Exec body:**
```json
{
  "command": ["bash", "-c", "echo hello"],
  "timeout": 30,
  "user": "ubuntu"
}
```

**File Push body:**
```json
{
  "path": "/home/ubuntu/file.txt",
  "content": "file contents or base64",
  "encoding": "text"
}
```

**Copy to Sandbox body:**
```json
{
  "source_path": "/home/ubuntu/project/app.py",
  "dest_path": "/home/ubuntu/test/app.py"
}
```

---

## Threads & Messaging

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/threads` | List threads (filterable) |
| POST | `/v1/threads` | Create a new thread |
| GET | `/v1/threads/:id` | Get thread details |
| GET | `/v1/threads/:id/messages` | Get all messages in a thread |
| POST | `/v1/threads/:id/messages` | Send a message (triggers agent response) |
| GET | `/v1/threads/:id/participants` | List thread participants |
| POST | `/v1/threads/:id/participants` | Add a participant (AGENT or USER) |
| DELETE | `/v1/threads/:id/participants/:member_id` | Remove a participant |

---

## Requests & Approvals

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/requests` | List requests (filterable by status, approver_type) |
| POST | `/v1/requests` | Create an approval request (auto-routed to approver) |
| POST | `/v1/requests/:id/approve` | Human operator approves a request |
| POST | `/v1/requests/:id/reject` | Human operator rejects a request |
| POST | `/v1/requests/:id/agent-approve` | Agent approves a subordinate's request |
| POST | `/v1/requests/:id/agent-reject` | Agent rejects a subordinate's request |

---

## Secrets Management

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/secrets` | List secret metadata (names and scopes, never plaintext values) |
| POST | `/v1/secrets` | Create an encrypted secret |
| DELETE | `/v1/secrets/:id` | Delete a secret |

**Create Secret body:**
```json
{
  "scope_type": "agent",
  "scope_id": "uuid",
  "name": "API_KEY",
  "value": "sk-live-..."
}
```
`scope_type` can be `"agent"`, `"manager"` (department), `"company"`, or `"holding"`. Agents fetch secrets via `GET /v1/agents/:id/secrets/:name` with hierarchical lookup (agent → manager → company → holding).

---

## Services & Engagements

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/services` | List available services |
| POST | `/v1/services` | Create a service offering |
| POST | `/v1/engagements` | Create a service engagement (auto-creates thread) |
| POST | `/v1/engagements/:id/activate` | Mark engagement as ACTIVE |
| POST | `/v1/engagements/:id/complete` | Mark engagement as COMPLETED |

---

## System Management

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/system/settings` | Get all system settings |
| PUT | `/v1/system/settings` | Update system settings |
| GET | `/v1/system/update-check` | Check for updates (stable/beta/dev channels) |
| POST | `/v1/system/update` | Trigger system update |
| GET | `/v1/system/containers` | List all Docker containers and status |
| GET | `/v1/system/containers/:id/logs` | Get container logs (tail-able) |

---

## Agent Daemon (called by VMs)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/agentd/register` | VM registers with control plane on boot |
| POST | `/v1/agentd/heartbeat` | VM sends periodic heartbeat |

---

## Scripts

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/scripts/install-openclaw.sh` | Serve OpenClaw install script for VM cloud-init |

---

## WebSocket

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/events` | Real-time event stream (company creation, VM provisioning, messages, approvals, file transfers, etc.) |
