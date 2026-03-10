# Security Architecture

## Tokens and Encrypted At Rest
Every API token, VM proxy token, and OpenClaw gateway access token is encrypted at rest using AES-GCM wrapping keys in postgres. MultiClaw derives its master encryption key from `/var/lib/multiclaw/master.key` (0600 root).

## Networking
- The OpenClaw API is bound to loopback `127.0.0.1:18789` on each VM.
- `multiclaw-agentd` initiates outbound connections to `multiclawd` only.
- The `ollama-proxy` binds to `0.0.0.0:11436` but drops packets not from the Incus subnet.

## Panic Operations (Quarantine)
The Quick-Panic button is a **kill switch** that fully silences an agent:

1. Sets the agent's status to `QUARANTINED` in the database.
2. Stops the agent's OpenClaw Docker container, terminating all in-progress processing.
3. Blocks all further DM delivery to and from the agent — any attempt returns `403 Forbidden`.
4. Broadcasts a `agent_quarantined` event to the UI.

Once quarantined, the agent cannot send or receive any messages. To understand what happened before quarantine, review the agent's conversation history in the **Agent Comms** page of the dashboard.

API: `POST /v1/agents/:id/panic`

## Secrets Management
MultiClaw provides an encrypted secrets store for sensitive values like API keys, database credentials, and service tokens. This is the **only** safe way to give agents access to credentials — never paste secrets into chat messages or DMs.

### Storing Secrets
Create a secret via the API:
```
POST /v1/secrets
{
  "scope_type": "agent",
  "scope_id": "<agent-uuid>",
  "name": "COINEX_API_KEY",
  "value": "your-secret-value",
  "description": "Full-access CoinEx API key for trading operations"
}
```
The `description` field is optional but recommended — it helps agents choose the right credential when they have multiple secrets for the same service (e.g., a read-only key vs a full-access key).

**Scope types:**
- `"agent"` — Available only to the specific agent.
- `"manager"` — Available to the specified manager and all workers in their department. Use this to give a team shared credentials (e.g. a read-only API key for the research team vs a full-access key for the trading team).
- `"company"` — Available to all agents in the company.
- `"holding"` — Available to all agents across all companies.

### How Agents Retrieve Secrets
Agents can list all secrets available to them via `GET /v1/agents/:id/secrets` (returns names and descriptions, never values). They fetch a specific secret's value by name via `GET /v1/agents/:id/secrets/:name`. The lookup is **hierarchical**: agent → manager (department) → company → holding. This lets you set a company-wide default, override it per-department, and override it again per-agent.

When an agent has multiple credentials for the same service, they are instructed to read the descriptions and choose the most appropriate one for the task at hand (e.g., using a read-only key for data fetching rather than a full-access key).

### Encryption
Secret values are encrypted at rest with AES-GCM using the same master key as API tokens. Plaintext values are never returned by `GET /v1/secrets` (which lists metadata only) — they are only decrypted when an agent fetches a specific secret by name.

### Secret Scrubbing
All stored messages (chat, DMs, approvals) are automatically scrubbed of known secret values before being written to the database. If an agent accidentally includes a secret in a message, the value is replaced before storage.

### Managing Secrets
- **List metadata**: `GET /v1/secrets` — returns names, scopes, and IDs (never values).
- **Delete**: `DELETE /v1/secrets/:id`

## DM Anti-Loop Protection
Agent-to-agent DMs support automatic multi-turn conversations where agents take turns responding. Multiple safeguards prevent these conversations from running away:

1. **Natural endings**: Each agent receives instructions to end the conversation naturally by signaling `[END_CONVERSATION]` when they have nothing more to add. The tag is stripped before storing messages — users never see it.
2. **Safety ceiling**: If an agent fails to signal completion, a hard limit of 20 turns stops the conversation to prevent truly runaway loops.
3. **Pair cooldown**: After a conversation between two agents completes, a 2-minute cooldown blocks new conversations between the same pair. This prevents agents from starting fresh conversations immediately after one ends.
4. **Quarantine checks**: Before each message in a conversation, both agents' quarantine status is checked. If either agent is quarantined mid-conversation, the conversation stops immediately.
5. **Rate limiting**: Agents are limited to 10 messages per minute per sender.
