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

## Messaging — Communicate with Other Agents

### Send a Direct Message to Another Agent
```bash
curl -s -X POST {{MULTICLAW_API_URL}}/v1/agents/{{AGENT_ID}}/dm \
  -H 'Content-Type: application/json' \
  -d '{"target": "TARGET_AGENT_ID_OR_HANDLE", "message": "Your message here"}'
```
The target agent will receive your message and respond. Use `@handle` (e.g. `@ceo-acme`) or a UUID.

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

## Important Notes

1. Replace `COMPANY_ID`, `CEO_NAME`, etc. with actual values.
2. Your agent ID is: `{{AGENT_ID}}`
3. Always check the response status. A 2xx status means success.
4. Company type MUST be uppercase `INTERNAL` or `EXTERNAL`.
5. All API responses are JSON. Use `python3 -m json.tool` to pretty-print if needed.
