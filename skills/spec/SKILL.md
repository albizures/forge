---
name: spec
description: Define a Forge Spec from settled grilling or Wayfinder findings, or create a published Spec's Tasks as a one-shot linear plan.
---

# Spec

Select the mode from the user's request, then read its companion document completely:

- **define** — synthesize settled discussion or Wayfinder findings into one published Spec: [define.md](define.md).
- **to tasks** — create the full linear Task plan for an existing published Spec, only when the user requests task creation: [to-tasks.md](to-tasks.md).

A published Spec is approved for planning. Merely naming a Spec or finding that it has no Tasks does not request task creation. If the intended mode or target is ambiguous, ask rather than choosing work to execute.

For an explicit request to define and create Tasks, run define first, then to tasks with the newly published Spec. A define-only request stops after publication.

## Shared boundaries

- Use [the Forge skill](../forge/SKILL.md) and its command reference for CLI revision/endpoint selection, Workspace and Project scope, relationships, and Record operations. The installed global CLI may be a different Forge Revision.
- Use the project's domain glossary and relevant ADRs. [The Skills glossary](../CONTEXT.md) defines this workflow's terms.
- Forge owns workflow Records and relationships; change them through its public commands, not backing storage or GitHub issues.
- These modes publish definitions and plans only. Stop after reporting the created Record IDs; Task execution, dispatch administration, worktrees, PR orchestration, and Spec completion belong outside this skill.
