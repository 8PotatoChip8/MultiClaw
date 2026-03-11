# Heartbeat Checklist

When the system sends a heartbeat, review this checklist. If everything is clear, respond with only `[HEARTBEAT_OK]`.

## Quick Checks
1. **Pending requests** — Call `GET /v1/requests?status=PENDING_APPROVAL` to check for unapproved requests. If any exist, review and approve/reject them.
2. **CEO status** — Call `GET /v1/agents` and check if all CEOs are active and healthy. Note any agents in unexpected states.
3. **Operator messages** — If the operator has sent you a message since your last check, address it.

## Rules
- Do NOT narrate what you are about to do. Just do the checks and report.
- If all checks pass with nothing to report, respond with exactly: `[HEARTBEAT_OK]`
- If something needs attention, handle it and briefly summarize what you did.
- Keep responses under 300 characters when possible.
