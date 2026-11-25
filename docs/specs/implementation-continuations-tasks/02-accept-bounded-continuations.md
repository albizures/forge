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

## Implementation evidence

- Status: complete (local source implementation only).
- Input commit: `73c088fbeb896f2d13291545b3d0ab66d0a7abe3`, including T01's verified
  output and committed local planning context, in the user-authorized `forge/`
  checkout. No managed Assignment or live execution authority was created.
- Output commit: `18dedd6423f76f541410d6059469f47d5bbedbe9`.
- Wire disposition: `{"continuation":{"stopping_reason":"...","completed_work":"...","verification":"...","remaining_work":"..."}}`.
  All four strings are required/nonblank; verification must truthfully identify
  actual results and unrun gates. `verdict: "fail"` is the existing non-complete
  wire sentinel, not an Assessment. Only the authenticated Task Implementation
  attempt may submit it, with no Finding/verification effects.
- Application owns handoff validation and limit policy. Persistence atomically
  records checkpoint/handoff, input adoption, attempt closure/result, counters and
  limit holds. No Check passes, no Assessment is inserted, no Finding/history or
  immutable Base Commit is rewritten. The successor's immutable attempt captures
  the accepted handoff and checkpoint. Existing retirement/reconciliation and
  dispatch gates remain authoritative.
- Tests exercise 19 changed checkpoints followed by the accepted/held twentieth;
  two unchanged checkpoints; unchanged/changed/unchanged; missing/blank content;
  dirty/mismatched/stale evidence; invalid/foreign credentials and attempts;
  Manual/review rejection; concurrent exact replay; conflicting replay; and
  replay after closure, restart and later body edits. Injected final-result-write
  failure rolls back checkpoint, counters and limit hold, including at exhaustion.
  A stronger existing safety diagnostic survives limit acceptance.
- Fake-provider integration runs ordinary daemon reconciliation into the same
  Implementation Check, delivers handoff context, retains capacity after uncertain
  cleanup or retained resources, and checks independent dependency, provider and
  hold blockers. Existing workspace tests retain mode/capacity gates. Mixed
  Continuation → interruption → substantive failure → pass → review handback
  verifies independent accounting and retention through freshness invalidation.
  Real-Git Assignment RPC tests accept committed B from input A and reject dirty
  checkpoints without adoption; CLI tests forward the explicit structured handoff
  using protected delivery and redact rejected provider messages.
- Compatibility: storage revision **15** adds `check_continuations`; ordinary
  opening rejects older stores before writes. Explicit isolated upgrades from
  supported schemas 7–14 add an empty table without inferred historical events;
  same-schema snapshots retain actual budgets, handoffs and replay inventories.
  Upgrade stamp failure rolls back atomically. Existing serialized dispositions
  remain accepted; added attempt/progress fields decode absent historical values
  as null/empty without rewriting stored JSON. Strict older clients may reject
  new response fields/disposition; documented client/server coordination is
  required. Storage, outcome and context contracts are documented.
- Red/green evidence: temporarily disabling checkpoint accounting/input adoption
  made `cargo test -p forge-store --test task_check_continuation deliberate_continuations`
  fail because accepted work still used A instead of checkpoint B. Restoring the
  acceptance call passes; the injected change was not committed.
- Focused verification passed: `cargo test -p forge-store`,
  `cargo test -p forge-store --test task_check_continuation`,
  `cargo test -p forge-server implementation_continuation_rpc`, and the focused
  deliberate-continuation test after its final additions. Final root gates passed:
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (**542 passed, 0 failed, 1 existing ignored**), and
  `git diff --check`. Initial fixture/lint failures were corrected before the
  final full run. The workspace suite includes CLI and public RPC regressions.
- Self-review against T02 and `CODING_STANDARDS.md`: no unresolved findings.
- Blockers: none for T02. Dedicated operator inspection/reset remains T03;
  interrupted-work recovery and profile rollout remain later Tasks. The existing
  ignored test was not run. No shipped skills changed, so skill parser/portability
  gates were not applicable. No live activation, migration, profile registration
  or selection, Check repinning, #37 recovery/dispatch, branch reset, merge or
  force-push was performed. Local verification is not release readiness.

## Routing hints

Application outcome/attempt contracts; persistence outcomes, task progress and attempt selection; Assignment context delivery; RPC/server/CLI outcome adaptation; explicit schema upgrade and storage README; `docs/dispatch.md` and `docs/assignment-context.md`.

## Boundaries

No new model-mediated progress evaluator, configurable limit system, reviewer handoff authority, live profile changes, or recovery of unexplained checkout changes. Keep extraction capability-focused and limited to enabling this slice.
