# T06 — Deliver stopping guidance, glossary, and operator documentation

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T05; Builds-On T05.** Use actual interfaces delivered by T01–T05, not placeholder command spellings.

Spec AC: **19**, final documentation/domain coverage of **13, 15–17, 20, 22**.

## Outcome

Agents and operators have accurate, testable instructions for continuing work, submitting each outcome, inspecting/resetting budgets and safely authorizing salvage. The proposed runtime glossary term is applied during implementation, not planning. No live profile or Task is changed.

## Scope

- Update Implementation profile source: keep implementing and testing until done or concretely constrained. Inspection, a small commit or remaining complexity alone is insufficient reason to stop.
- Distinguish success, substantive failure, deliberate Continuation, actual interruption and human input. Show actual supported submission shape; require constraint, completed work, truthful verification/unrun gates and remaining work for handoffs.
- Explain clean checkpoint acceptance, same-Check continuation, ceilings 20/two unchanged, operator inspection/reset, safe retirement, and recovery of unaccepted committed/dirty/untracked work. Never describe evidence change as certified semantic progress.
- Add the parent Spec's exact proposed Continuation block to `crates/CONTEXT.md`. Do not create an unagreed ADR or duplicate the glossary into operational policy prose.
- Update command examples and affected assignment-context, dispatch, worktree and recovery documentation against the implemented contracts. Retain storage/wire compatibility explanations and explicit offline upgrade constraints.
- Profile source currently lives at Factory Workspace `.agents/profiles/implementation.md`, outside the Forge Repository. Deliver its revised source as a tracked repository artifact at `docs/profiles/implementation-continuations.md`, preserving the existing source's resolved runtime/frontmatter policy unless separately authorized. Record the originating Workspace source and reviewable diff in that artifact or adjacent handoff notes. Do not overwrite the external source, register/select it, or imply that Git inheritance transfers an external file. This artifact is a delivery package, not an automatically effective profile.
- Include a rollout handoff listing separate approvals needed for revision installation/activation, explicit state migration if required, profile registration/selection, supported handling of existing immutable Check pins, and eventual #37 recovery. Do not invent a supported repin operation if none exists.

## Observable acceptance and verification

1. The delivered profile renders/resolves with the intended existing runtime/model policy and new outcome instructions using isolated validation fixtures only.
2. Parser/contract tests validate all documented CLI and outcome examples; malformed and role-inappropriate examples are not presented as valid.
3. The runtime glossary has the exact approved block; documentation matches actual reset effects, budget persistence and recovery authority.
4. The profile source artifact and handoff notes are tracked and reviewable. Successors need no untracked or external file to understand or verify the delivery.
5. Tests or a focused checklist demonstrate instructions do not equate unfinished work with interruption, do not recommend trivial-commit handoffs, and distinguish accepted checkpoints from interrupted salvage. Stochastic agent behavior is supplemental evidence only.
6. No live registration, selection, activation, migration, Check repin or #37 mutation has occurred. Rollout remains explicitly separate.

Run shared root gates and affected documented-command/profile tests; run skill portability gates only if shipped skills are actually changed. Report actual commands, compatibility notes and unrun checks.

## Routing hints

`docs/profiles/`, actual Workspace Implementation profile source (read-only input for this Task), `crates/CONTEXT.md`, `docs/assignment-context.md`, `docs/dispatch.md`, `docs/assignment-recovery.md`, `docs/worktrees.md`, profile resolution/parser tests and affected CLI documentation tests.

## Boundaries

Do not change registered immutable profile revisions or existing Check pins. A delivered source file is not evidence of effective live behavior. No model change, new skill wrapper or live rollout is included.
