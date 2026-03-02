# Approvals and Agent Hierarchy

## Escalation Policy

### CEOs
- Up to 2 CEOs are allowed per company.
- Adding a 2nd CEO requires User approval (`ADD_SECOND_CEO`).

### Managers
- CEO may hire up to 2 Managers without approval.
- A 3rd Manager requires MainAgent approval.
- 4+ Managers require User approval (escalated via MainAgent).

### Workers
- Manager may hire up to 3 Workers without approval.
- A 4th Worker requires CEO approval.
- A 5th Worker requires MainAgent approval.
- 6+ Workers require User approval.

## Approvals Process
An un-approved request is generated as a `PENDING` request. When the Approver chain executes the final `APPROVE` or `REJECT`, the system finalizes the state.
