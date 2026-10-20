---
name: spec
description: Orchestrate a Forge Spec from definition through Task planning and delivery.
disable-model-invocation: true
---

# Spec

Use this orchestration skill as the user-facing workflow for taking work through a Forge **Spec**. Existing Spec execution remains the default. A human may explicitly opt a confirmed Spec into worktree-backed implementation mode during Task planning; that mode changes how ordinary child Tasks are later executed, not how Specs are defined. Select exactly one workflow branch, then read its companion document before acting:

- Current conversation or request to define work → `to-spec.md`
- Existing Spec and request to plan its Tasks, including an explicit worktree-mode opt-in request → `to-tasks.md`
- Existing Spec and request to execute a child Task → `work-spec.md`

Use `/forge` as the command reference for project scope, relationships, readiness, lifecycle, comments, and completion. Forge is authoritative; do not edit backing store files directly.

## Route the request

1. Identify the referenced Forge record, if any, and inspect it when needed.
2. Choose the branch from the request and record state using the table above.
3. Read the selected companion document completely before acting.
4. Preserve the phase boundary: define first, plan under an existing Spec, then work a child Task.

If the request is ambiguous between planning and execution check with `forge tree <spec id>` and if there is available tasks, work in the first one.

## Shared guards

- `/spec` is the only user-facing entry point for this workflow; branch documents are internal companions.
- A definition request publishes exactly one Spec and no child Tasks unless the user explicitly requests both definition and planning; worktree mode is not implied during Spec definition.
- Task planning requires an existing, confirmed Spec parent; never create orphan Tasks. Worktree-backed implementation requires explicit human opt-in at this boundary before implementation starts.
- Task execution selects only a direct child Task of the named Spec and resolves at most one child per session.
- The parent Spec is never auto-completed by Task work.
- Keep Spec and Task relationships in Forge, and use only Forge public commands.
