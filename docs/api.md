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
| POST | `/v1/companies/:id/ledger` | Create a ledger entry |
| GET | `/v1/companies/:id/balance` | Get balance breakdown by currency |
| GET | `/v1/companies/:id/orders` | List trade orders (filterable by status, symbol) |
| POST | `/v1/companies/:id/orders` | Create a trade order |
| PATCH | `/v1/companies/:id/orders/:order_id` | Update order (fill, cancel, etc.) |
| GET | `/v1/companies/:id/positions` | Get current trading positions |
| GET | `/v1/companies/:id/budget` | Get company budget summary |

**Create Ledger Entry body:**
```json
{
  "type": "CAPITAL_INJECTION",
  "amount": 50000,
  "currency": "USD",
  "memo": "Initial funding",
  "counterparty_company_id": "uuid",
  "engagement_id": "uuid"
}
```
Types: `CAPITAL_INJECTION`, `REVENUE`, `EXPENSE`, `INTERNAL_TRANSFER`. Currency is any string (USD, EUR, BTC, ETH, etc.). For `INTERNAL_TRANSFER`, the counterparty receives a paired `REVENUE` entry automatically.

**Balance response:** `{ "USD": { "revenue": 0, "expenses": 0, "capital": 50000, "net": 50000 } }`

**Hire CEO body:**
```json
{
  "name": "Agent Name",
  "specialty": "domain expertise",
  "preferred_model": "minimax-m2.7:cloud"
}
```
`preferred_model` is optional — overrides the system default.

**Create Order body:**
```json
{
  "agent_id": "uuid",
  "exchange": "binance",
  "symbol": "BTC/USD",
  "side": "BUY",
  "order_type": "LIMIT",
  "quantity": 0.5,
  "price": 45000.00,
  "quote_currency": "USD"
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
| POST | `/v1/agents/:id/restart` | Restart agent's OpenClaw container |
| GET | `/v1/agents/:id/health` | Check agent container health |
| GET | `/v1/agents/:id/queue` | Get agent's message queue status |
| GET | `/v1/agents/:id/thread` | Get or create a DM thread with this agent |
| POST | `/v1/agents/:id/dm` | Send agent-to-agent DM (auto-conversation loop) |
| POST | `/v1/agents/:id/dm-user` | Agent sends a message to the human operator |
| POST | `/v1/agents/:id/send-file` | Send a file to another agent (policy-checked, max 10 MB) |
| GET | `/v1/agents/:id/file-transfers` | List file transfers involving this agent |
| GET | `/v1/agents/:id/threads` | Get all threads this agent participates in |
| GET | `/v1/agents/:id/recent-messages` | Get agent's recent messages across all threads |
| GET | `/v1/agents/:id/memories` | List agent memories/knowledge base |
| POST | `/v1/agents/:id/memories` | Create or update an agent memory |
| DELETE | `/v1/agents/:id/memories/:mid` | Delete a memory entry |
| GET | `/v1/agents/:id/openclaw-files` | Read agent's OpenClaw workspace files |
| POST | `/v1/agents/:id/knowledge` | Push knowledge content to agent's workspace |
| GET | `/v1/agents/:id/secrets` | List secrets accessible to this agent (names and descriptions, never values) |
| GET | `/v1/agents/:id/secrets/:name` | Fetch a secret by name (hierarchical lookup) |

**Agent-to-Agent DM body:**
```json
{
  "target": "uuid-or-handle",
  "message": "text content"
}
```
The `target` field accepts a UUID or an agent handle (e.g., `@ceo-acme`). The sender is the agent in the URL path (`:id`). DM conversations auto-loop until agents naturally conclude the discussion. A safety ceiling of 20 turns prevents runaway loops, and a 2-minute cooldown between the same pair prevents re-initiation. Both agents' quarantine status is checked before each message.

**Send File body:**
```json
{
  "target": "uuid-or-handle",
  "src_path": "/path/on/sender/vm",
  "dest_path": "/path/on/receiver/vm",
  "encoding": "text"
}
```
`dest_path` and `encoding` are optional.

**Hire Manager/Worker body:**
```json
{
  "name": "Agent Name",
  "specialty": "domain expertise",
  "preferred_model": "minimax-m2.7:cloud"
}
```
`preferred_model` is optional — overrides the system default.

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
  "command": "echo hello",
  "user": "employee",
  "working_dir": "/home/employee",
  "timeout_secs": 30
}
```
`user` defaults to `"employee"` (UID 1000). `working_dir` defaults to `/home/employee`. `timeout_secs` defaults to 30.

**File Push body:**
```json
{
  "path": "/home/employee/file.txt",
  "content": "file contents or base64",
  "encoding": "text"
}
```

**Copy to Sandbox body:**
```json
{
  "source_path": "/home/employee/project/app.py",
  "dest_path": "/home/employee/test/app.py"
}
```

---

## Shared VMs

Shared VMs are company-scoped Incus VMs accessible to multiple agents within a company or department.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/shared-vms` | List shared VMs (filterable by `company_id`, `vm_purpose`) |
| POST | `/v1/shared-vms` | Provision a shared VM |
| GET | `/v1/shared-vms/:id` | Get shared VM details |
| DELETE | `/v1/shared-vms/:id` | Destroy a shared VM |
| POST | `/v1/shared-vms/:id/start` | Start a shared VM |
| POST | `/v1/shared-vms/:id/stop` | Stop a shared VM |
| POST | `/v1/shared-vms/:id/rebuild` | Rebuild a shared VM |
| POST | `/v1/shared-vms/:id/exec` | Execute a command on a shared VM |
| GET | `/v1/shared-vms/:id/info` | Get shared VM state and IP |
| POST | `/v1/shared-vms/:id/file/push` | Write a file to a shared VM |
| POST | `/v1/shared-vms/:id/file/pull` | Read a file from a shared VM |

**Provision Shared VM body:**
```json
{
  "requester_agent_id": "uuid",
  "company_id": "uuid",
  "vm_purpose": "development",
  "department_manager_id": "uuid",
  "label": "Team Dev Server",
  "resources": {}
}
```
`department_manager_id`, `label`, and `resources` are optional.

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
| GET | `/v1/secrets/audit` | Get secret access audit log |
| DELETE | `/v1/secrets/:id` | Delete a secret |

**Create Secret body:**
```json
{
  "scope_type": "agent",
  "scope_id": "uuid",
  "name": "API_KEY",
  "fields": [
    { "label": "Access Key", "value": "AKIA..." },
    { "label": "Secret Key", "value": "wJalr..." }
  ],
  "description": "Read-only API key for market data"
}
```
For single-value secrets, a legacy `"value": "..."` field is also accepted instead of `"fields"`.

`scope_type` can be `"agent"`, `"manager"` (department), `"company"`, or `"holding"`. The `description` field is optional but recommended — it helps agents choose the right credential when they have multiple secrets for the same service. Agents can list their accessible secrets via `GET /v1/agents/:id/secrets` (names and descriptions only) and fetch by name via `GET /v1/agents/:id/secrets/:name` with hierarchical lookup (agent → manager → company → holding).

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

## Meetings

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/meetings` | List meetings |
| POST | `/v1/meetings` | Create a meeting |
| GET | `/v1/meetings/:id` | Get meeting details |
| POST | `/v1/meetings/:id/close` | Close a meeting |

**Create Meeting body:**
```json
{
  "topic": "Q1 Strategy Review",
  "organizer_id": "uuid",
  "participant_ids": ["uuid", "uuid"],
  "scheduled_for": "2026-03-25T14:00:00Z"
}
```
`scheduled_for` is optional.

---

## System Management

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/system/settings` | Get all system settings |
| PUT | `/v1/system/settings` | Update system settings |
| GET | `/v1/system/holding` | Get current holding config (name, main agent, model, initialized status) |
| POST | `/v1/system/reset` | Wipe all data and reinitialize holding (stops all containers, truncates tables) |
| GET | `/v1/system/update-check` | Check for updates (stable/beta/dev channels) |
| POST | `/v1/system/update` | Trigger system update |
| GET | `/v1/system/containers` | List all Docker containers and status |
| GET | `/v1/system/containers/:id/logs` | Get container logs (tail-able) |

**System Reset body:**
```json
{
  "holding_name": "My Holding",
  "main_agent_name": "KonnerBot",
  "default_model": "minimax-m2.7:cloud"
}
```
All fields are optional — defaults are used if omitted. This stops all OpenClaw containers, clears in-memory state, truncates all database tables, and reinitializes with a fresh holding and MAIN agent.

**Holding Config response:**
```json
{
  "holding_name": "My Holding",
  "main_agent_name": "KonnerBot",
  "default_model": "minimax-m2.7:cloud",
  "initialized": true
}
```

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

## Models

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/models` | List available Ollama models and the system default |
| GET | `/v1/models/pull-status` | Check pull/download progress for all models |
| POST | `/v1/models/pull` | Pull/download a model to Ollama |

**Pull Model body:**
```json
{
  "model": "minimax-m2.7:cloud"
}
```

**List Models response:**
```json
{
  "models": ["minimax-m2.7:cloud", "qwen3-coder:480b-cloud", "kimi-k2.5:cloud"],
  "default": "minimax-m2.7:cloud"
}
```

---

## AI Rewrite

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/rewrite` | Rewrite a draft message for clarity using an AI model |

**Rewrite body:**
```json
{
  "text": "hey can u check on the thing we talked about",
  "model": "minimax-m2.7:cloud"
}
```
`model` is optional — defaults to the `rewrite_model` system setting, or the system default model.

**Response:**
```json
{
  "rewritten": "Could you check on the item we discussed earlier?"
}
```

---

## World

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/world/snapshot` | Get a full snapshot of the current world state (companies, agents, threads) |

---

## WebSocket

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/events` | Real-time event stream (company creation, VM provisioning, messages, approvals, file transfers, etc.) |
