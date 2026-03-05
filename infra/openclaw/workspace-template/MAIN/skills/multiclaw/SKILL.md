---
name: multiclaw
description: MultiClaw platform operations — MainAgent capabilities
---

# MultiClaw Platform Operations — MainAgent

You are the **MainAgent** running on the MultiClaw platform. You have full authority over holding-level operations.

## API Base URL

```
{{MULTICLAW_API_URL}}
```

All requests should include `Content-Type: application/json`.

## Your Operations

### List All Agents
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

### Hire a CEO for a Company
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/hire-ceo \
  -H 'Content-Type: application/json' \
  -d '{"name": "CEO_NAME", "specialty": "DESCRIPTION"}'
```

### View Company Org Tree
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/org-tree
```

### View Financial Ledger
```bash
curl -s {{MULTICLAW_API_URL}}/v1/companies/COMPANY_ID/ledger
```

### Provision a VM Workstation
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/vm/provision
```

## Important Notes

1. Replace `COMPANY_ID`, `CEO_NAME`, etc. with actual values.
2. Your agent ID is: `{{AGENT_ID}}`
3. Always check the response status. A 2xx status means success.
4. Company type MUST be uppercase `INTERNAL` or `EXTERNAL`.
5. All API responses are JSON. Use `python3 -m json.tool` to pretty-print if needed.
