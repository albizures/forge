# Safe Implementation Continuations and Interrupted-Work Recovery Specification

## Problem Statement

An agent process ending is not the same as a Task being complete. Implementation agents sometimes stop with useful partial work, while the bound Check remains unmet. Operators need Forge to distinguish a deliberate handoff from substantive failure, actual interruption, and a request for human input.

Task #37 exposed two problems. Agents stopped prematurely without identifying a concrete execution constraint. A later agent committed partial work and submitted an interrupted outcome. Forge accepted the submitted changed evidence but retained the previous expected commit; subsequent dispatch then classified the acknowledged commit as unexpected. Repeated retries alone cannot fix this semantic mismatch.

Operators should not have to discard useful work, bypass ownership protections, or silently rewrite evidence to get execution moving. They also need protection against agents endlessly handing off inspection-only work or trivial commits.

## Solution

Introduce an explicit **Continuation** outcome for Implementation agents. It deliberately hands unfinished work to a replacement agent for the same Check, without passing that Check or treating the handoff as substantive failure. A Continuation contains a reason, completed-work summary, actual verification, remaining work, and a clean, independently verified repository commit.

Forge atomically accepts that checkpoint as successor input and records the handoff before retiring the current Assignment. Dispatch remains subject to confirmed retirement, resource reconciliation, capacity, dependencies, Dispatch Holds, and existing readiness rules.

Bound this normal handoff path separately from remediation and interrupted retries: hold after 20 accepted Continuations for a Check, or after two consecutive Continuations whose commit evidence is unchanged. Retain the checkpoint that reaches a limit. A changed commit is not a certification of meaningful progress.

Keep genuine interruptions separate. Changes without an accepted checkpoint are preserved but are not automatically adopted as trusted successor input. An operator-authorized recovery path may make retained work available to a replacement Implementation agent after safe ownership transfer, without passing any Check.

This is a local Spec for Project `factory/forge`, approved for planning by the user's request to capture the settled discussion. It is not a Forge Record and does not authorize live activation or recovery.

## User Stories

1. As an operator, I want agent-process completion distinguished from Task completion, so that an unfinished Task is never accidentally marked Done.
2. As an Implementation agent, I want an explicit deliberate-handoff outcome, so that partial work can continue without pretending it failed or was interrupted.
3. As a replacement Implementation agent, I want the accepted checkpoint as my input, so that I do not repeat or lose prior work.
4. As a replacement Implementation agent, I want a handoff reason and remaining-work summary, so that I can resume with useful context.
5. As an operator, I want verification results distinguished from unrun gates, so that a checkpoint does not overstate readiness.
6. As an operator, I want agents instructed to keep working rather than stop after inspection or a small commit, so that Continuation is not the default work rhythm.
7. As an operator, I want checkpoint acceptance and successor input recorded atomically, so that an acknowledged commit does not become unexplained checkout drift.
8. As an operator, I want replacement dispatch delayed until old-agent cleanup is safe, so that two agents cannot modify the same work concurrently.
9. As an operator, I want a finite continuation budget independent of remediation, so that legitimate handoffs do not consume failure allowances.
10. As an operator, I want repeated unchanged handoffs stopped early, so that inspection-only loops do not run indefinitely.
11. As an operator, I want a total continuation limit even when commits change, so that trivial-commit loops remain bounded.
12. As an operator, I want the checkpoint that reaches a limit retained, so that stopping dispatch does not lose accepted work.
13. As an operator, I want counters durable across workflow changes and restarts, so that incidental actions cannot renew execution allowance.
14. As an operator, I want explicit, explained budget resets, so that intervention is attributable and auditable.
15. As an agent, I want an actionable rejection for a changed-commit interruption submission, so that I can distinguish deliberate handoff from recovery.
16. As an operator, I want committed and uncommitted interrupted work preserved, so that unexpected termination does not destroy deliverables.
17. As an operator, I want an authorized salvage path for unreported work, so that a successor can inspect and continue without silently certifying it.
18. As a reviewer, I want review and publication authority unchanged, so that Continuation does not grant permission to modify deliverables.
19. As an operator, I want exact outcome retries to be safe, so that a lost response does not duplicate a handoff or spend budget twice.
20. As an operator, I want delivery separated from live rollout, so that immutable profile pins and retained resources are not changed without approval.

## Acceptance criteria

1. An explicit Continuation disposition is available only to a Task's bound Implementation agent. It is distinguishable from success, substantive Implementation failure, interruption, and human input. Forge does not infer the disposition from free-form prose.
2. An accepted Continuation leaves the bound Check unpassed and the Task unfinished. The next execution remains an attempt of that same Check rather than advancing to review or publication.
3. A Continuation requires a nonblank stopping reason identifying a concrete execution constraint, completed-work summary, actual verification results including unrun gates, and remaining-work summary. Missing required handoff content is rejected without partial acceptance.
4. A Continuation requires a clean Managed Worktree and a full commit identifying the actual independently observed output in the bound Repository. Dirty, mismatched, stale, cross-owner, or unauthorized evidence cannot become continuation input.
5. Acceptance atomically records the checkpoint as successor input, accepted handoff, durable counters, and any limit-triggered hold. No accepted-checkpoint state exists in which the same unchanged checkout is subsequently rejected merely because the prior expected commit was retained.
6. Acceptance does not itself authorize replacement. Forge retires the old Assignment and reconciles all retained resources before a successor may act. Uncertain cleanup retains the existing safety blockers and capacity protections across restart.
7. After safe retirement, eligible replacement dispatch uses the accepted checkpoint and makes the accepted handoff context available. Existing dependency, mode, capacity, readiness, provider, and Dispatch Hold gates still apply.
8. An exact submission retry returns its original accepted result without duplicating the checkpoint or incrementing counters twice, including after response loss or restart. Conflicting retries are rejected without partial effects; replay does not restore mutation authority.
9. Each accepted Continuation increments the same Check's total continuation count once. The twentieth accepted Continuation retains its checkpoint and applies a durable hold preventing further dispatch until explicit operator intervention.
10. Each accepted Continuation compares its commit evidence with its bound input. Unchanged evidence increments the consecutive-unchanged count; changed evidence resets that count. The second consecutive unchanged-evidence Continuation retains its checkpoint and applies a durable hold.
11. Continuation totals are independent of interruption and remediation budgets. Continuation counters survive retry, restart, remediation, evidence invalidation, and Task body edits. None of these actions silently renews continuation allowance.
12. An explicit operator reset requires nonblank attribution and explanation, durably records the reset, and is the only way to clear the total continuation count. Ordinary hold clearing, remediation reset, or retry does not bypass an exhausted continuation limit. Reset does not erase historical outcomes or confer recovery/ownership authority.
13. Operator-facing inspection exposes continuation totals, consecutive-unchanged counts, accepted checkpoint/handoff context, limit diagnostics, and reset history through public interfaces with useful human and structured output. Evidence change is reported as change, not as certified semantic progress.
14. A substantive failed Implementation attempt remains distinct from Continuation and follows existing failure/remediation semantics. Review and publication do not gain the new handoff permission or authority to modify deliverables.
15. An interruption submission containing a changed commit is rejected with an actionable diagnostic explaining deliberate Continuation versus interrupted-work recovery. The submission is not acknowledged as an accepted interrupted outcome, does not silently adopt the commit, and does not discard checkout changes.
16. Genuine interruption with no accepted checkpoint preserves committed, uncommitted, and unreported changes as recoverable evidence. Forge does not automatically turn those changes into trusted successor input or infer that an old agent is absent from a timeout or missing heartbeat.
17. A supported, explicitly authorized operator recovery path can make retained work the starting point for a replacement Implementation agent only after establishing that the old agent cannot act and retained resources are safe. The replacement can inspect and salvage retained work without losing uncommitted changes. Recovery passes no Check and certifies neither correctness nor completion.
18. Recovery and ordinary continuation preserve the immutable Stack Base Commit, existing evidence history, Findings, and required completion gates. They do not implicitly reset branches, force-push, discard changes, skip Checks, or complete the parent Spec.
19. The Implementation profile explicitly instructs agents to keep implementing and testing until completion or a concrete constraint. Initial inspection, a small successful commit, or remaining complexity alone are insufficient stopping reasons. It distinguishes Continuation, substantive failure, actual interruption, and human-input outcomes and requires truthful verification reports.
20. Changed public outcome contracts, diagnostics, inspection, reset and recovery operations are documented and tested for compatibility. Any breaking storage changes advance the schema revision and follow explicit Revision State upgrade rules; ordinary opening never performs implicit conversion, invented historical backfill, or counter reset.
21. Implementation delivery includes regression tests reproducing #37's changed-evidence interruption behavior, atomic continuation acceptance, bounded execution, restart/replay behavior, and safe salvage. Required repository verification results and unrun gates are reported truthfully.
22. Delivery does not install or activate a Forge Revision, migrate live state, register/select revised profiles, repin existing Checks, or recover/dispatch #37. Those are separate, explicitly approved operational steps using supported interfaces.

## Implementation Decisions

- Introduce explicit Implementation Continuation semantics rather than overloading interruption or substantive failure. Exact serialized names and public command spellings are implementation details to establish and document; this Spec does not claim they already exist.
- Domain behavior belongs in the application layer; durable evidence, counters, outcome replay, and atomic acceptance belong in persistence; wire contracts belong in RPC; server and CLI adapt those contracts. External Git and provider observations remain behind existing integration boundaries and outside database writer transactions.
- Use the actual clean output commit as the accepted checkpoint. Continuation changes current work input, not the immutable Stack Base Commit. Acceptance is not a passing Assessment or a successful review.
- Validate authoritative state when committing acceptance. Preserve existing server-side capability scope, immutable Assignment identity, freshness, ownership, and replay rules.
- Record continuation limits per stable Check identity. Keep total and consecutive-unchanged counters separate from remediation and interruption counters. Limits are initially 20 total and two consecutive unchanged-evidence Continuations; no operator configurability is required by this definition.
- A changed commit resets only the consecutive-unchanged count, not the total. Do not introduce model-mediated semantic-progress judging. The total ceiling bounds changed-but-useless loops; completion Checks establish acceptance.
- Budget exhaustion retains the accepted checkpoint and blocks further dispatch. Ordinary resume must not silently reset continuation counters or bypass exhausted limits. Preserve existing safety diagnostics rather than overwriting them with a weaker reason.
- Interrupted changed evidence must not be accepted through a path that leaves the old expected commit authoritative. Reject that submission with actionable guidance rather than silently converting the agent's declared outcome.
- Retained interrupted changes are not an accepted checkpoint. Recovery requires explicit operator authorization and safe ownership transfer; the replacement Implementation agent may inspect and salvage the retained state without claiming that it is already valid output.
- Keep normal continuation, interrupted recovery, and rollout authority separate. Neither checkpoint acceptance nor operator reset establishes safe replacement by itself.
- Update Implementation profile source and supporting documentation. Registered profile revisions and Check pins remain immutable; making new behavior effective for existing Tasks is a separate supported and approved operation.
- Assess storage and wire compatibility explicitly. Do not invent historical Continuations from old failed or interrupted outcomes. Follow current explicit upgrade contracts for any required schema change.

## Testing Decisions

Test observable public behavior and durable invariants, not private function shapes. Use isolated Repositories, stores, listeners, and provider fakes. Prefer controlled failure injection and synchronization to sleeps. Exercise default-parallel tests. Test actual dispatch/outcome/recovery boundaries as well as focused persistence policy.

Known testing seams include Check-bound outcome acceptance and replay, work-state inspection, durable progress counters, Assignment retirement/resource reconciliation, Git evidence observation, RPC/CLI authorization and presentation, and explicit offline storage upgrade behavior. Existing outcome, cumulative-gate, task-progress, interrupted retry, and recovery tests provide prior art; they are not proof that new Continuation semantics already exist.

### Test plan by acceptance criterion

1. Submit each disposition through the public Assignment interface; verify explicit selection and reject Continuation for review, publication, unrelated ownership, and non-Task contexts.
2. Accept a Continuation and verify no passing verdict or Done transition; after cleanup verify the same Check is selected.
3. Omit each required handoff field independently; assert rejection and no accepted checkpoint/counter effects. Verify complete context round-trips.
4. Test clean output, dirty checkout, mismatched commit/Repository, stale freshness, and cross-owner submission; assert no unauthorized input adoption.
5. Reproduce a changed checkpoint, accept it, reopen the store and reconcile; verify expected input matches the retained checkout. Inject acceptance failures and verify checkpoint, counters, handoff and limit hold commit together or not at all.
6. Simulate uncertain provider cleanup, restart and retained resources; verify no duplicate acting agent and no premature capacity release.
7. Verify the successor receives the checkpoint and handoff. Independently block dependency, capacity, mode, provider readiness and hold gates and assert that none is bypassed.
8. Lose the acceptance response and replay before/after restart and closure; verify the same result and one increment. Reject a conflicting replay without mutation.
9. Accept 19 Continuations, then the twentieth; verify retained twentieth checkpoint and dispatch hold. A blocked redispatch cannot create a twenty-first automatic Continuation attempt.
10. Test unchanged/unchanged and unchanged/changed/unchanged sequences against each attempt's bound input. Verify the correct count, reset and second-unchanged hold.
11. Verify continuation accounting independently of failed Implementation, interrupted retry, review handback, restart, body edit and evidence invalidation; no incidental path resets counters.
12. Reject blank reset attribution/explanation; verify durable audit and reset effects. Demonstrate ordinary resume/remediation reset cannot renew exhausted allowance and reset cannot bypass retained ownership/resources.
13. Exercise human and JSON inspection with active, exhausted and reset histories. Verify precise changed/unchanged wording rather than a semantic-progress claim.
14. Confirm existing substantive failure consumes remediation allowance, whereas Continuation does not; regression-test unchanged review/publication authority and gates.
15. Reproduce the reported changed-commit interruption submission. Verify actionable rejection, no accepted interrupted result, preserved clean changed checkout, and no silent adoption. Confirm legitimate unchanged interruption still uses its established path.
16. Terminate or retire an agent before submission with clean changed HEAD, dirty tracked files, and untracked work. Verify all retained state remains intact and no automatic adoption occurs.
17. Exercise recovery denied before safe retirement, denied with unknown resources, and authorized after reconciliation. Verify replacement inspection/salvage retains committed and uncommitted work without a passing verdict.
18. Assert Stack Base Commit and history are unchanged through continuation, limit hold, rejection and salvage. Verify Findings and required completion/publication gates remain enforced.
19. Test profile resolution and documented-command examples against the supported new interfaces; review the rendered instructions for outcome distinctions and stopping rules. A stochastic agent run is supplementary, not the sole acceptance test.
20. Test public contract compatibility and structured diagnostics. If storage changes, test old-store rejection without writes, isolated explicit upgrade, reopening and counter/history preservation without invented historical events.
21. Run repository-required formatting, workspace lint/test and diff checks; include affected profile/skill command-parser or portability gates when applicable. Report failures and unrun gates rather than claiming release readiness.
22. Verify the deliverable contains source/tests/docs only, with no live activation, profile registration/selection, Check repinning, state migration or Task recovery actions.

## Domain changes

### Glossary

Target: `crates/CONTEXT.md`, Forge Runtime Language section.

**Add:**

```markdown
**Continuation**:
A deliberate handoff of unfinished Implementation work for another Agent to continue without passing the bound Check.
_Avoid_: interruption, implementation failure, Task completion
```

Do not apply this glossary block while defining or planning this Spec.

### ADRs

None. No separate ADR was agreed during the design discussion. Existing ownership, evidence-preservation, and daemon-first architectural decisions remain applicable.

## Out of Scope

- Review or Publication Continuations, or new reviewer authority to mutate deliverables.
- Automatically declaring partial checkpoints correct or marking a Check passed.
- Automatic adoption, deletion, reset, or repair of unreported interrupted changes.
- Automatic semantic-progress judging or one-small-commit-per-agent as the default execution model.
- Guaranteed detection of dishonest or semantically trivial work before the total continuation ceiling.
- Changing the existing remediation/interruption budgets or inventing historical continuation events.
- Replacing the target Task's product scope, implementing its Base Branch Policy feature, or completing its parent Spec.
- Stack repinning, branch resets, force-push, merge, skipped Checks, or weakened ownership/capability rules.
- Installing/activating revisions, migrating live state, registering/selecting profiles, changing existing Check pins, or recovering/dispatching #37 as part of implementation delivery.
- Creating Forge Records or Tasks while Forge is unreliable; this local file is the definition artifact.

## Notes

### Incident evidence and provenance

Source: the user-approved design discussion following investigation of Task #37. This conversation has no separate Forge provenance Record; do not invent a `generated-by` relationship.

- Pi session: `01a1e7af-4c8c-75d7-a88d-17ef14e738fd`.
- Assignment: `1a1e7af4a49-5`; Implementation Check: `117`.
- Session bound input: `e4c472d17d041a3074ac3014c6adb79ea209e7ee`.
- Session committed output: `4a29bbeabd98feb5b5da75277e692ead3fc18f97`.
- Immutable Stack Base Commit: `104c84b15a0cbb95dda7db126baca1efe53ed128`.
- The agent submitted changed output as `fail / interrupted`. Acceptance returned the new evidence but retained old work input. Subsequent dispatch held for the same new commit as unexpected evidence.
- The observed session contained no visible tool failure, timeout, or user interruption explaining its premature stopping. This observation does not establish why the model chose to stop.
- The checkout was clean with both partial implementation commits retained at investigation time. Historical facts are not current ownership or live recovery authorization; re-inspect before any operational recovery.

### Compatibility and delivery constraints

Exact API names, recovery representation for dirty/untracked work, and explicit upgrade mechanics must be resolved against current interfaces during implementation. Preserve the acceptance behaviors above; do not turn those representation choices into silent policy changes. Material contradictions return to the human.

Existing registered profiles are immutable snapshots. Editing profile source alone does not update the behavior pinned by existing Task Checks. Rollout must explicitly account for this without unsupported repinning or persistence edits.

Relevant skills:

- `forge` — supported interfaces, Assignment ownership, dispatch and safe recovery boundaries.
- `domain-modeling` — apply the proposed glossary terminology during authorized implementation.
- `review-implementation` — verify the delivered behavior against this Spec and its eventual Tasks.
- `review-coding-standards` — check architecture, atomicity, compatibility, recovery and verification rules.

Relevant files (routing hints, not fixed implementation prescriptions):

- `CODING_STANDARDS.md` — architecture, transactions, recovery, compatibility and verification requirements.
- `GLOSSARY-MAP.md` and `crates/CONTEXT.md` — domain vocabulary and proposed glossary target.
- `crates/app/src/outcomes.rs` and `crates/app/src/task_attempts.rs` — application outcome/attempt contracts.
- `crates/store/src/outcomes.rs` — current interrupted acceptance and evidence handling.
- `crates/store/src/task_progress.rs` — existing durable progress and budget policy.
- `crates/store/src/task_attempts.rs` — Check-bound attempt persistence.
- `crates/rpc/`, `crates/server/`, and `crates/cli/` — public wire and adapter behavior.
- `crates/store/README.md` and `docs/state-upgrades.md` — storage contracts and explicit upgrades.
- `docs/dispatch.md`, `docs/assignment-recovery.md`, `docs/assignment-context.md`, and `docs/worktrees.md` — execution and recovery contracts.
- `crates/docs/adr/0001-daemon-first-interfaces.md`, `crates/docs/adr/0005-autonomous-task-dispatch-recovery.md`, and `crates/docs/adr/0008-operator-selected-dispatch-workspaces.md` — applicable architectural history; consult current contracts where historical decisions have been superseded.
- Workspace profile source `.agents/profiles/implementation.md` (relative to the Factory Workspace, outside this Repository) — stopping guidance; inspect the actual pinned revision separately before any rollout.
