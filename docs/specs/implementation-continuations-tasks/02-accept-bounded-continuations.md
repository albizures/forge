# T02 — Accept bounded Implementation Continuations end to end

## Authority and input

Parent: [local Spec](../implementation-continuations.md). Read the [shared execution contract](README.md#shared-execution-contract). Project: `factory/forge`. **Depends on T01; Builds-On T01.** Inherit T01's verified repository output only.

Spec AC: **1–11**, continuation aspects of **14, 18, 20–21**. T03 adds the operator reset/inspection slice; do not expose unbounded Continuations while waiting for it.

## Outcome

An authenticated Implementation agent can submit a deliberate clean checkpoint through the public Assignment interface. Forge accepts it atomically, closes the attempt without passing the Check, safely retires the old Assignment, and supplies that checkpoint and handoff context to the next eligible attempt of the same Check. Limits work from the first supported Continuation.

## Scope

- Add an explicit Continuation disposition and required stopping reason, completed-work summary, actual verification/unrun gates, and remaining-work summary. Choose and document exact wire representations; do not infer intent from prose.
- Allow only the bound Task Implementation role. Preserve failure, interruption, human-input, review and publication semantics.
- Independently observe a clean worktree and the actual full output commit in the bound Repository. Validate scope, ownership, freshness and atomic mutable preconditions.
- Persist accepted checkpoint as successor input, handoff context, attempt result and counters together. Leave the Check unpassed; do not record success, advance to review, rewrite Stack Base Commit, or clear Findings.
- Count accepted Continuations per stable Check identity: total ceiling 20; consecutive-unchanged ceiling two. Compare against that attempt's input; changed evidence resets only the consecutive-unchanged count.
- The limit-reaching checkpoint is accepted and retained with a durable dispatch hold. Resume, remediation, retries, restart, invalidation and body edits cannot bypass allowance. Keep counters independent from existing interruption/remediation counters and preserve stronger existing safety diagnostics.
- Integrate ordinary continuation selection and context delivery with existing Assignment retirement, resource reconciliation, capacity and readiness gates. Lost cleanup responses do not authorize replacement or release unknown capacity.
- Exact outcome replay returns the original result and spends budget once, including after closure/restart. Conflicting/stale/foreign submissions have no partial effects.
- Implement required schema/explicit upgrade support now rather than deferring compatibility for new persisted state. Do not infer historical Continuations from old outcomes.

## Observable acceptance and verification

1. Public submit → acceptance → confirmed cleanup → successor bootstrap demonstrates changed checkpoint inheritance and full handoff context on the same Check.
2. All missing-field, dirty/mismatched evidence, cross-owner, stale, review/publication, and conflicting-replay cases reject without partial checkpoint adoption.
3. Simulated transaction failure leaves no partially accepted checkpoint/counter/hold. Reopen and exact replay preserve the accepted result without duplicate increments.
4. Nineteen accepted changed-evidence Continuations can continue; the twentieth retains its checkpoint and blocks further dispatch. Two consecutive unchanged-evidence Continuations stop earlier. An unchanged/changed/unchanged sequence resets only the unchanged count.
5. Existing dependencies, mode, provider, capacity, holds and uncertain resources independently block replacement. No duplicate agent can act through this new path.
6. Interrupted retries, substantive failure and review handback retain their own accounting. Body edits, invalidation, restart and ordinary resume cannot renew continuation totals.
7. Isolated storage compatibility/upgrade tests preserve actual history and reject unsupported stores before writes; no live-state migration occurs.

Use existing outcome replay, task-progress, cumulative-gate, Check-attempt and provider-fake test seams. Tests must fail without the new behavior. Run shared root gates; report actual evidence and unrun checks.

## Routing hints

Application outcome/attempt contracts; persistence outcomes, task progress and attempt selection; Assignment context delivery; RPC/server/CLI outcome adaptation; explicit schema upgrade and storage README; `docs/dispatch.md` and `docs/assignment-context.md`.

## Boundaries

No new model-mediated progress evaluator, configurable limit system, reviewer handoff authority, live profile changes, or recovery of unexplained checkout changes. Keep extraction capability-focused and limited to enabling this slice.
