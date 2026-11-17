# T04 — Authorize salvage of clean retained commits

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T03; Builds-On T03.** Inherit T03's verified output.

Spec AC: clean-commit slice of **16–18**, recovery aspects of **20–21**. T05 expands the same supported recovery workflow to dirty and untracked changes; this Task must reject those states non-destructively until then.

## Outcome

After safe old-agent retirement, an explicitly authorized operator can permit a replacement Implementation agent to inspect and salvage a clean retained commit that has no accepted checkpoint. This recovery authority is distinct from normal Continuation and from passing a Check.

## Scope

- Add or extend supported operator app/RPC/CLI recovery interfaces for the actual interrupted-work case: an existing In Progress Task with Check-bound Assignment history and retained evidence. Do not assume existing recovery for an unclaimed Open Task provides this authority.
- Require explicit authorization, verify canonical worktree/Repository/Task identity, and establish that the old agent cannot act and retained resources are safe. Preserve uncertainty rather than inferring absence from a timeout, closed attempt or missing heartbeat.
- Persist sufficient authorization, selected retained evidence and diagnostics to restart safely and prevent duplicate salvage launches. External observations must be reconciled with atomic mutable ownership/precondition checks.
- Let the next authorized attempt of the same Implementation Check start from retained work with recovery context. Do not rewrite historical accepted input or pretend the unreported commit was an accepted Continuation.
- Keep the immutable Stack Base Commit, Findings and completion gates. Recovery does not pass Checks, certify implementation correctness, implicitly reset budgets, or itself waive ordinary dispatch eligibility.
- Handle operator request retries/conflicts safely; if observations change or outcome is unknown, retain work and diagnostics rather than adopt, discard, reset or force-push it.
- Document clean-only support at this stage and how it differs from ordinary checkpoint acceptance and preexisting worktree-claim recovery.

## Observable acceptance and verification

1. A fixture matching #37's unexplained clean changed HEAD is blocked from ordinary adoption; an authorized recovery after verified retirement makes it available to a replacement Implementation agent without a passing verdict.
2. Deny recovery before safe retirement, with retained/unknown resources, wrong scope, wrong Repository/worktree or changed observations; all evidence remains intact.
3. Repeated/restarted operator recovery does not create duplicate authority or acting agents. Conflicting requests do not partially change selected input.
4. Stack Base Commit, historical outcome evidence, Findings, progress budgets and later review/publication gates remain intact.
5. Dirty and untracked fixtures are preserved and rejected with a diagnostic pending T05, never cleaned to satisfy this Task's narrower slice.
6. Isolated public-interface and persistence tests demonstrate authorization, no partial acceptance on failure, reopening and ordinary dispatch-gate enforcement.

Run the shared verification gates and document compatibility effects and actual unrun checks. Use fixtures, never live #37 state.

## Routing hints

`recover_managed_worktree` and existing Open-Task eligibility are prior art, not sufficient semantics. Inspect application worktree/dispatch recovery boundaries, provider resource reconciliation, store attempts/work state, worktree adapter recovery validation, RPC/CLI recovery endpoints, and `docs/assignment-recovery.md`.

## Boundaries

No automatic unexplained-commit adoption, historical Continuation backfill, Check pass, branch manipulation, old-agent capability reuse, or live operation. T05 supplies dirty/untracked salvage rather than weakening these guarantees.
