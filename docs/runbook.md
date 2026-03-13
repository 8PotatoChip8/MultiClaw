# Runbook

## Host Level Checks
### Are the Host services running?
```bash
sudo systemctl status multiclaw-stack
docker compose -f /opt/multiclaw/docker-compose.yml ps
sudo systemctl status ollama
```

### Rotating Master Key
See internal CLI: `multiclaw init rotate-keys`

## VM Level Checks
```bash
incus list  # show all VMs
incus shell mc-3a96c961  # shell into a desktop VM (mc-<agent-uuid-prefix>)
incus shell mc-3a96c961-sb  # shell into a sandbox VM
```
VM names use the first 8 characters of the agent's UUID: `mc-{uuid-prefix}` for desktop, `mc-{uuid-prefix}-sb` for sandbox.

## Quarantining an Agent
If an agent is behaving dangerously or looping, use the Quick-Panic button in the UI or call the API:
```bash
curl -X POST http://127.0.0.1:8080/v1/agents/<agent-id>/panic \
  -H "Authorization: Bearer <token>"
```
This will:
1. Set the agent's status to `QUARANTINED`.
2. Stop their OpenClaw Docker container (fully silencing them).
3. Block all DM delivery to and from the agent.

After quarantining, review the agent's conversation history in **Agent Comms** to understand what happened.

## Checking Agent DM Conversations
Use the **Agent Comms** page in the dashboard to view all agent threads and messages.

Via API:
```bash
# List an agent's threads
curl http://127.0.0.1:8080/v1/agents/<agent-id>/threads \
  -H "Authorization: Bearer <token>"

# Read messages in a thread
curl http://127.0.0.1:8080/v1/threads/<thread-id>/messages \
  -H "Authorization: Bearer <token>"
```

If agents are stuck in a DM loop, quarantine one of them (see above). The 2-minute per-pair cooldown should prevent most loops automatically, but quarantine is the guaranteed kill switch.

## MainAgent Heartbeat
The MainAgent (KonnerBot) performs periodic check-ins to review pending approvals, company status, and any issues that need attention. By default, this runs every 10 minutes.

### Change heartbeat interval
```bash
curl -X PUT http://127.0.0.1:8080/v1/system/settings \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"heartbeat_interval_secs": "600"}'
```
Default: `600` (10 minutes). Set to `0` to disable the heartbeat entirely.

If KonnerBot has nothing to report, the heartbeat costs very little (a short prompt + a `[HEARTBEAT_OK]` response that is not stored). Reports are only generated and posted to the DM thread when something needs attention.

The heartbeat loop waits for OpenClaw container recovery to complete (signaled via a watch channel), then waits an additional 5 minutes for post-restart recovery prompts to settle before starting.

## Post-Restart Recovery Prompts
After a restart, multiclawd sends role-appropriate recovery prompts to all active agents in hierarchical order (MAIN → CEO → MANAGER → WORKER, 60s between tiers). This tells agents the system restarted and asks them to check memory and resume work.

### Disable recovery prompts
```sql
INSERT INTO system_meta (key, value) VALUES ('recovery_prompts_enabled', 'false')
  ON CONFLICT (key) DO UPDATE SET value = 'false';
```

### Check recovery prompt status in logs
```bash
docker compose logs multiclawd | grep -i "recovery prompt"
```

## Ollama Concurrency
multiclawd gates concurrent LLM requests through a semaphore. On startup it probes Ollama with 10 test requests to auto-discover the limit.

### Check current concurrency config
```bash
docker compose exec multiclawd env | grep CONCURRENT
# or check logs:
docker compose logs multiclawd | grep -i "concurrency"
```

### Adjust concurrency
Set `OLLAMA_NUM_PARALLEL` on the Ollama service and `MULTICLAW_MAX_CONCURRENT_OLLAMA` in the multiclawd environment. They should match.
Default: 4. Set in `/etc/systemd/system/ollama.service.d/concurrency.conf` and `/opt/multiclaw/.env`.

## Message Queue Troubleshooting

### Check for stuck items
```sql
SELECT id, agent_id, kind, status, claimed_at, retry_count, error
FROM message_queue WHERE status = 'PROCESSING'
ORDER BY claimed_at ASC;
```
Items in PROCESSING for more than 5 minutes will be automatically recovered by the stale claim sweep (runs every 60s). Items are also subject to a 300-second per-handler timeout — if a handler hangs, the item is marked as failed and retried.

### Manually recover a stuck item
```sql
UPDATE message_queue SET status = 'PENDING', claimed_at = NULL
WHERE id = '<item-uuid>' AND status = 'PROCESSING';
```

### Check failed items
```sql
SELECT id, agent_id, kind, error, retry_count, max_retries
FROM message_queue WHERE status = 'FAILED'
ORDER BY completed_at DESC LIMIT 20;
```

### Check queue depth per agent
```sql
SELECT agent_id, COUNT(*) as pending
FROM message_queue WHERE status = 'PENDING'
GROUP BY agent_id ORDER BY pending DESC;
```

### Check logs for timeouts
```bash
docker compose logs multiclawd | grep "timed out after 300s"
```

## Provisioning Secrets for Agents
Store API keys, credentials, and other sensitive values using the Secrets API. **Never paste secrets into chat messages or DMs** — use the secrets store instead.

### Create a secret
```bash
curl -X POST http://127.0.0.1:8080/v1/secrets \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "scope_type": "agent",
    "scope_id": "<agent-uuid>",
    "name": "API_KEY",
    "value": "your-secret-value",
    "description": "Full-access API key for trading operations"
  }'
```
The `description` field is optional but recommended — it helps agents choose the right credential when multiple secrets exist for the same service.

**Scope types:**
- `agent` — Only the specified agent can access it.
- `manager` — The manager and all workers in their department can access it.
- `company` — All agents in the company can access it.
- `holding` — All agents across all companies can access it.

Agents retrieve secrets by name. The lookup is hierarchical: agent → manager (department) → company → holding. This lets you set defaults at the company level, override per-department, and override per-agent.

### Create a department-scoped secret
```bash
curl -X POST http://127.0.0.1:8080/v1/secrets \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "scope_type": "manager",
    "scope_id": "<manager-uuid>",
    "name": "EXCHANGE_API_KEY",
    "value": "your-department-api-key",
    "description": "Read-only exchange API key for market data research"
  }'
```
This gives the manager and all their workers access to the secret. Other departments in the same company won't see it.

### List secrets (metadata only)
```bash
curl http://127.0.0.1:8080/v1/secrets \
  -H "Authorization: Bearer <token>"
```

### Delete a secret
```bash
curl -X DELETE http://127.0.0.1:8080/v1/secrets/<secret-id> \
  -H "Authorization: Bearer <token>"
```
