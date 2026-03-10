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

Agents also receive a `MEMORY.md` template (seeded with agent identity on first creation, preserved on respawn) and a `TOOLS.md` describing available tools including `memory_search` and `memory_get` for semantic recall.

## Onboarding
When an agent hires a new employee, they brief the new hire via DM — explaining the company context, role expectations, and immediate tasks. This behavior is defined in the SOUL.md operating principles for MAIN, CEO, and MANAGER roles.

## Agent Computers
Every agent receives two Incus VMs:
- **Desktop VM**: A persistent workstation for ongoing work. Retains all software, files, and configuration across reboots.
- **Sandbox VM**: A wipeable testing environment for running untrusted code or experiments. Can be rebuilt at any time via `POST /v1/agents/:id/vm/rebuild` without affecting the desktop.

Agents can copy files between their desktop and sandbox using `POST /v1/agents/:id/vm/copy-to-sandbox`.

## Inter-Agent File Sharing
Agents can send files to other agents via `POST /v1/agents/:id/send-file`. Policy rules enforce hierarchical access:
- Agents can send files to peers within the same company and to their direct reports.
- Cross-company file sharing routes through the MAIN agent.
- Maximum file size: 10 MB.

File transfers are logged and visible via `GET /v1/agents/:id/file-transfers`.
