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
incus list multiclaw
incus shell mc-mainhc-ceo-1
# from inside the VM
systemctl status openclaw-gateway
systemctl status multiclaw-agentd
```

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
    "value": "your-secret-value"
  }'
```

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
    "value": "your-department-api-key"
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
