# Heartbeat Checklist

When the system sends a heartbeat, review this checklist. If everything is clear, respond with only `[HEARTBEAT_OK]`.

## Quick Checks
1. **Pending requests** — Call `GET /v1/requests?status=PENDING_APPROVAL` to check for unapproved requests. If any exist, review and approve/reject them.
2. **CEO status** — Call `GET /v1/agents` and check if all CEOs are active and healthy. Note any agents in unexpected states.
3. **Operator messages** — If the operator has sent you a message since your last check, address it.
4. **CEO activity check** — Review the CEO Activity Report appended to this heartbeat (if present).
   - For any **EXTERNAL** company CEO idle for more than 20 minutes: DM them to check on progress and remind them to keep driving their company forward. Ask what they are working on, what blockers they have, and what their next steps are.
   - For **INTERNAL** company CEOs: being idle is acceptable if they are waiting for work from sister companies. Only check in if idle for more than 60 minutes.
   - Do NOT check in on CEOs who were recently active (idle < 20 min).
   - Check in on at most 2 CEOs per heartbeat to avoid flooding.

## Rules
- Do NOT narrate what you are about to do. Just do the checks and report.
- If all checks pass with nothing to report (and no CEO check-ins needed), respond with exactly: `[HEARTBEAT_OK]`
- If something needs attention, handle it and briefly summarize what you did.
- Keep responses under 300 characters when possible (unless you needed to DM CEOs).
