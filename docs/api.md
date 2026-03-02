# MultiClaw API Server (multiclawd)

## Base URL
Default local binding: `http://127.0.0.1:8080/v1/`

## Authorization
All non-init endpoints require a Bearer token generated at install and sent as `Authorization: Bearer <token>`.

## Core Endpoints
- **POST /v1/install/init** - Initializes the system state.
- **POST /v1/companies** - Create a company.
- **GET /v1/companies/:id/org-tree** - Gets the company hierarchy.
- **POST /v1/companies/:id/hire-ceo** - Starts the CEO hiring workflow.
- **POST /v1/agents/:id/hire-manager** - Starts the manager hiring workflow.
- **POST /v1/agents/:id/hire-worker** - Starts the worker hiring workflow.
- **POST /v1/threads/:id/messages** - Create a message and dispatch it to an agent.
- **POST /v1/requests/:id/approve** - Manually approve a pending request.
- **POST /v1/requests/:id/reject** - Manually reject a pending request.
- **WS /v1/events** - WebSocket endpoint for streaming real-time event updates to UI.
