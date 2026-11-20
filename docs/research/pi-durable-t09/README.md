# T09 — Conservative supervised retirement

## Status: complete for the controlled T09 deliverable

The existing operator retirement path now revokes durable Assignment authority
before identity-bound cancellation, supervised process cleanup and conservative
resource reconciliation. T10 may proceed; **production durable dispatch and
writable/automatic recovery remain disabled**. No live Forge Records, profiles,
provider accounts or operator worktrees were changed. Packaging, automatic
scheduling and real-provider release verification remain later Tasks.

## Implemented boundary

1. `Application::retire_agent_assignment` installs the hold and persistent
   `retirement-pending/operator` marker, then closes/revokes before adapter I/O.
   Its atomic finalizer cannot accept adapter success with unresolved resources.
2. `durable_execution::retirement` owns the typed original operation, owner proof,
   known-admission/revocation gates and exact resource-discharge policy. A new
   operation must match ownership in the retained authorization grant. A fresh
   directly spawned lookalike cannot substitute for the original executor.
3. Store revision **22** retains one immutable retirement operation per runtime and
   an immutable reconciliation timestamp. Immediate transactions scope-authorize
   before replay and commit accepted reconciliation/resource retirement together.
   A runtime resource trigger requires accepted proof plus revoked authority.
   Failure rolls back both. No historical receipt, owner or grant is invented.
4. The server controller retains only directly spawned `Child` handles and their
   original private control pipes. It journals intent before one nonblocking
   control delivery. Partial/blocked delivery is terminal uncertainty, not replay.
   Live ownership failure terminally fences the controller. The default server
   factory routes durable cleanup separately, delegates Herdr/Grilling to their
   original adapters, and still rejects new durable dispatch/recovery.
5. The dedicated Linux supervisor becomes a subreaper before spawning its worker.
   A strictly versioned private retirement control binds the full original Launch,
   operation and owner. It fences grants, verifies live ownership and persists host
   intent before TERM. The runner's TERM handler closes authorization and aborts
   guarded execution. After five seconds the host can attempt group KILL only
   while original leader/group ownership remains verified.
6. The host drains its complete child set, including adopted escaped descendants.
   Only kernel `ECHILD`, unchanged protected ownership/storage and successful
   synced receipt persistence establish its bounded descendant-cleanup proof.
   Signals, abort acknowledgement, leader exit and lease expiry are insufficient.
   An escaped/surviving child, identity drift or failed host intent/receipt write
   retains uncertainty and the host flock; no orphan PID is adopted or chased.
7. Forge reads the exact protected receipt under exclusive inspection, validates
   the original lock identity and performs a fresh matched read-only committed
   effect scan before committing reconciliation. Unsettled effects are retained
   before execution resources can retire. Missing/incompatible inspection/runtime
   evidence keeps the operation pending even when process cleanup succeeded.
8. Only the bound runtime, known authorization-liveness causes and its matching
   missing-Outcome diagnostic can retire. Effect blockers, unknown provisioning,
   unrecognized diagnostics, other identities, holds and all history remain.
   Closed retries reconcile original evidence/timestamps; accepted replay works
   without runtime files or components. Restarted hosts deny grants when retirement
   intent exists. No worktree/index repair, runtime receipt repair, command replay,
   accepted Outcome mutation, resource deletion or recovery permission is added.

The 9-second live-host observation bound is separate from read-only effect
inspection (existing 30-second deadline plus bounded inspector kill). A repeat
cannot concatenate a partial pipe write, adopt a missing controller's process,
waive unknown-resource confirmation or turn restored evidence into signal authority.

## Compatibility and documentation

- Ordinary schema-21 opening fails before initialization writes. Explicit offline
  21 → 22 adds an empty retirement journal and replaces the runtime capacity guard;
  old table values are preserved and unresolved execution still blocks transfers.
- Runtime SQLite schema remains **2**, runner request wire remains **v1**. The new
  private retirement-control message has its own version **1**, strict fields and
  a bounded frame. No operator RPC method, CLI argument or public JSON field changes.
- No dependency was added. The implementation reuses Rustix subreaper/wait APIs,
  existing flock/ownership evidence, protected pipe delivery and read-only effect
  inspection. No npm/PATH component discovery was added to product code.
- Updated the recovery runbook, store/upgrade contracts, runner notes and the
  retirement portion of actual **ADR-0012**. The Spec's proposed ADR-0013 number
  was provisional. Writable recovery portions remain T10; no glossary was changed.

## Verification matrix

| Evidence | Observed behavior |
| --- | --- |
| Actual app dispatch → Managed Worktree → protected bootstrap → Harness/Bash/CLI Outcome → public CLI/RPC retirement | Seven parallel supervised scenarios, including a live original-host retirement. Missing profile source/model-key files do not prevent cleanup. Retained provisioning intent still blocks replacement after process proof; explicit public unknown-resource confirmation and retirement retry are required. Retry preserves accepted evidence, Check attempt, runtime identity, Git state and hold; no successor or second provision occurs. |
| Real SQLite/app/controller → private pipe → host → pinned Node consumer | Nine retirement tests cover long-running model/tool execution, delayed group shutdown, identity mismatch/restoration, unknown/unavailable launch control, failed Forge intent/result commits, host intent/receipt write failures, escaped-child survival, corrupt receipt and replaced-lock rejection, and controller restart reconciliation. |
| Real committed runtime effect inspection after owned shutdown | Unsettled canonical runtime tool intent is retained as an effect blocker before process reconciliation can release capacity; runtime bytes remain unchanged. Missing inspector preserves pending proof, then matched read-only retry retains the effect and the original operation. These are real runtime storage fixtures, not an additional external publication experiment. |
| Git/runtime preservation | Actual Git repository with dirty tracked, staged and untracked files: file bytes, index bytes and porcelain-v2 state unchanged. Canonical SQLite history and original owner evidence unchanged. This inert delayed-process fixture uses a protocol-fake effect observer, explicitly not a claim that its sentinel database is a compatible pi-durable store. |
| Store transaction/replay policy | Wrong Workspace, live authority, unknown admission, missing/original-owner mismatch, conflicting operations, invalid proof, immutable history, resource-write rollback and exact retry across reopen. Effects, unknown resources, foreign diagnostics and holds stay retained. Host proof is simulated at this persistence port, not inferred from it. |
| Real-process sticky signal regression | Corrupt start identity, persist `term-unverified`, restore evidence, observe `kill-unverified` and a live original worker. Removing the terminal fence makes this test fail with `kill-sent`; restoring the fix passes. |
| Private protocol/runner tests | Single-write partial/blocked retirement delivery never retries; strict unknown-field rejection; actual synthetic runner retirement retains committed history; mismatched owner/Launch cannot cancel or signal; a fresh host cannot grant after retirement intent. |
| Existing Herdr/Grilling and operator regressions | Original backend, preflight, ownership, cleanup, scope, capacity and confirmation behavior remain covered by the workspace suite. |

The session fixture's noncanonical `controlled.sqlite` cases use a deliberately
injected **protocol-fake** effect inspector. They prove real cancellation/process
behavior and Forge transactions, not canonical runtime inspection. The separate
canonical effect tests and supervised CLI tracer use the actual matched Node
inspector. All models/credentials are controlled fakes; no real-provider readiness
is inferred.

## Final checks

From the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

All passed on the corrected code snapshot: **705 Rust harness tests plus all
7 custom-harness supervised scenarios; 1 pre-existing opt-in probe ignored**.
Runner typecheck and **54 Node tests** passed. `git diff --check` passed.
The final workspace run used default parallelism. No clean-install,
packaged-component or real-provider release gate ran.

Earlier partial-increment workspace runs had two transient `hold_replacement`
failures; focused and later full default-parallel reruns passed. The historical
`durable_check_bootstrap`/supervisor timeout is also not diagnosed or claimed
resolved. During this implementation, fixture/schema expectations and a test that
seeded runtime bytes before fresh admission failed and were corrected; successful
reruns do not convert those earlier failures into evidence of a product fix.

## Reviews

Implementation review against T09 and AC10–12/AC21: the original retirement path,
revocation ordering, identity-bound cancellation, bounded owned-group cleanup,
verified descendant absence, retained uncertainty, restart reconciliation,
unknown-resource rules, preservation tests and retirement ADR portions are now
implemented. No recovery, replay, deletion or rollout is introduced. Review found
and corrected two safety gaps before completion: scan/retain committed effects
before capacity release, and require original grant ownership rather than a
newly spawned lookalike's receipt. Summary: no remaining T09 acceptance gaps;
T10 is separate authorized writable recovery, not implicit permission here.

Coding-standards review: policy/classification is in app, transactional enforcement
in store, wire definitions in RPC, and filesystem/process/provider I/O in adapters.
No I/O occurs inside database transactions. Exclusive inspection spans effect
observation through result commit; operation/result history and failures retain
uncertainty. Schema compatibility and explicit upgrades are tested. No new secrets,
dependencies or unsafe shell invocations were added. Summary: no introduced
coding-standards violations found on the corrected snapshot. These local reviews
do not replace T18's later independent review.

## Known supervision limits

This is Linux/local-filesystem, trusted-user supervision, not hostile same-user
containment. Subreaper proof covers descendants of this dedicated host, not remote
side effects or work delegated to unrelated services. External-effect inspection
and explicit operator confirmations remain independent. A daemon/controller crash
without a valid original receipt cannot manufacture cleanup permission; retained
capacity is intentional. Process-crash tests do not establish physical power-loss
durability. Production durable dispatch remains disabled until later release gates.
