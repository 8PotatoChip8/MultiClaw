# Agent Roles and Permissions

MultiClaw enforces a strict hierarchical corporate structure for its autonomous agents. Their authority and context are defined by tailored "Brain Files" injected into their OpenClaw workspace templates (`SOUL.md`, `AGENTS.md`, and `skills/multiclaw/SKILL.md`).

## The Hierarchy

### 1. MainAgent (MAIN)
- **Position**: Top of the hierarchy, overseeing the entire `Future Holdings` platform. Reports only to the human operator.
- **Authority**: Full holding-level operations.
- **Can**: Create companies (`INTERNAL` or `EXTERNAL`) and hire CEOs.
- **Cannot**: Be managed or bypassed.

### 2. Chief Executive Officer (CEO)
- **Position**: Top executive of a specific company. Reports to the MainAgent.
- **Authority**: Full company-level operations.
- **Can**: Hire managers to run departments, hire workers for specific tasks, view the company's ledger and org tree.
- **Cannot**: Create new companies or hire other CEOs.

### 3. Manager (MANAGER)
- **Position**: Middle management running a specific department or functional area. Reports to the CEO.
- **Authority**: Department-level execution and delegation.
- **Can**: Hire workers to complete tasks.
- **Cannot**: Create companies, hire CEOs, or hire other managers.

### 4. Worker (WORKER)
- **Position**: Individual contributor executing specific tasks. Reports to their manager (or CEO if hired directly).
- **Authority**: Task execution only. No management authority.
- **Can**: View the org tree, submit requests.
- **Cannot**: Hire anyone, create companies, or manage operations.

## Template Injection
When a new agent is hired, the `multiclawd` control plane looks in `infra/openclaw/workspace-template/` to render their workspace.
Since the `feat: role-specific agent brain files` update, it checks for `<ROLE>` specific subdirectories first (e.g., `CEO/SOUL.md` or `WORKER/skills/multiclaw/SKILL.md`) before falling back to generic templates. This ensures agents do not see API capabilities or instructions that exceed their authority.
