---
name: multiclaw
description: Interact with the MultiClaw platform to manage companies, agents, and resources
---

# MultiClaw Platform Operations

You are an agent running on the MultiClaw platform. Use `bash` with `curl` to interact with the
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

- `INTERNAL` = wholly-owned subsidiary
- `EXTERNAL` = external partner or client company

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

### Provision a VM Workstation
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/provision
```

### View Financial Ledger
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/ledger
```

### Submit a Request to Superior
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/requests \
  -H 'Content-Type: application/json' \
  -d '{"type": "ACTION", "requester_id": "{{AGENT_ID}}", "payload": {"description": "WHAT_YOU_NEED"}}'
```

## Important Notes

1. Replace `COMPANY_ID`, `MANAGER_NAME`, etc. with actual values.
2. Your agent ID is: `{{AGENT_ID}}`
3. Always check the response status. A 2xx status means success.
4. When hiring, the new agent automatically gets their own OpenClaw instance.
5. Company type MUST be uppercase `INTERNAL` or `EXTERNAL` — any other value will be rejected.
6. All API responses are JSON. Use `python3 -m json.tool` to pretty-print if needed.
