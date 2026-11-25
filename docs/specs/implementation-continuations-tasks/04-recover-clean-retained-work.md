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

## Implementation evidence

- Status: complete (local source implementation only).
- Input commit: `bae4cf550d4ce979dc0cc920bde0d11a30cb01d3`, containing T03's
  implementation and verification evidence, in the user-authorized `forge/`
  checkout. No managed Assignment or Forge Record was created.
- Output commit: `44e019ae0d9b4af58a2a0efff68a354b7c193baf`.
- Public operation: `forge dispatcher recover-retained-work <task> --workspace <id>
  --source-assignment <assignment> --generation <current-generation>
  --repository <registered-project-root> --commit <full-clean-HEAD>
  --actor <operator> --message <explanation> --confirm-old-agent-cannot-act
  --confirm-resources-safe [--json]`, through selected operator RPC
  `record.recoverRetainedWork`. Both confirmations and nonblank attribution are
  required. Assignment capabilities have no corresponding operation.
- Recovery requires an In Progress, single-Project Dispatched Task, its latest
  Check-bound Assignment, an unpassed Implementation Check with no accepted
  non-interruption outcome, verified explicit retirement markers, and fully
  reconciled ownership/resources. Git validation independently verifies canonical
  placement, actual Repository, deterministic Task Branch, immutable Base Commit
  ancestry, cleanliness and selected commit. Git observations remain outside the
  writer transaction; mutable scope/source/claim/ownership/freshness are rechecked
  atomically. Operators must exclude arbitrary external filesystem/Git writers;
  neither provider cleanup nor an assertion proves universal process death.
- Authorization advances freshness, invalidates current passes/attempts, selects
  retained evidence as salvage input and records immutable attributable authority
  with a retained hold in one transaction. It does not accept a Continuation,
  pass a Check, certify correctness, clear holds, reset budgets or launch an agent.
  Historical outcomes, Assessments, Findings and immutable Stack pins remain.
  Ordinary evidence reconciliation no longer automatically selects unexplained
  observed commits; clearing its hold cannot adopt them.
- Work inspection exposes `recoveries`; the next eligible same-Check attempt
  captures `bound.checkAttempt.recovery`, including across a body-edit freshness
  advance when selected input remains unchanged. Historical handoff context remains
  distinct. Exact retries, including after restart/replacement, return the original
  audit; conflicting requests fail without partial effects. Claim/dispatch still
  enforce ordinary holds, resource, provider and other eligibility gates.
- Compatibility: schema **17** adds immutable `retained_work_recoveries` keyed by
  source Assignment. Ordinary older-store opening rejects before writes; explicit
  isolated upgrades support schemas 7–17, preserve prior typed values and add no
  invented authorization. Added `WorkState.recoveries` and `Attempt.recovery` fields
  default empty/null when absent from historical stored JSON; original accepted
  responses are not rewritten. Strict older clients must upgrade with the server.
  Management Protocol is unchanged. Storage and upgrade docs are updated.
- Verification includes real-Git public RPC and persistence/application fixtures:
  live-owner and retained/unknown-resource denial; closed is not retired; wrong
  Workspace, source, freshness, claim, Repository, branch, redirected path and
  selected evidence; dirty tracked and untracked preservation/rejection; injected
  audit failure rolling back input/freshness; concurrent exact authorization;
  conflicting retries; reopening; same-Check successor context; retained prior
  continuation accounting/checkpoint/outcome replay; base preservation; holds and
  provider readiness blocking replacement; revoked old capabilities and no duplicate
  launches. Schema-16 rejection preserves source bytes; explicit upgrade preserves
  history with empty authority; injected schema-17 stamp failure rolls back DDL.
  Documented commands parse, and CLI parsing requires both confirmations.
- Final required root gates passed: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
  (**550 passed, 0 failed, 1 existing ignored**), and `git diff --check`.
  Initial fixture, diagnostic, parser and lint failures were corrected before the
  final run. No shipped skills changed; skill-only gates were not applicable.
  The existing ignored test was not run. Local checks are not release readiness.
- Implementation and coding-standards self-review: no unresolved findings.
- Blockers: none for T04. Dirty/untracked salvage remains T05 and is rejected
  non-destructively here. No live installation/activation, state migration,
  profile registration/selection, Check repinning, #37 recovery/dispatch, branch
  reset, merge or force-push was performed. Tests use isolated fixtures only.

## Routing hints

`recover_managed_worktree` and existing Open-Task eligibility are prior art, not sufficient semantics. Inspect application worktree/dispatch recovery boundaries, provider resource reconciliation, store attempts/work state, worktree adapter recovery validation, RPC/CLI recovery endpoints, and `docs/assignment-recovery.md`.

## Boundaries

No automatic unexplained-commit adoption, historical Continuation backfill, Check pass, branch manipulation, old-agent capability reuse, or live operation. T05 supplies dirty/untracked salvage rather than weakening these guarantees.
