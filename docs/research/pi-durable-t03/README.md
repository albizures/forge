# T03 — durable runtime identity and admission evidence

## Disposition

T03 implemented and locally verified. T04 may build on this persistence/wire
boundary. No real runner launch, live-provider request, Forge Record mutation,
profile migration, default-backend change or production rollout occurred.
This is deterministic application/store/wire evidence, not release readiness.

## Delivered interfaces

- `crates/app/src/durable_execution.rs`: immutable `Launch`, explicit `Backend`,
  `Runtime`, `Admission` alternatives; launch validation and result reconciliation
  policy; `Repository`/`Transport` ports and the `admit` application orchestration.
- `crates/store/src/durable_execution.rs`: immediate-transaction identity reservation,
  initial-send claim, exact result replay and authorized result persistence.
- `crates/rpc/src/durable_execution.rs`: runner protocol **1**, runtime schema **2**,
  exact component negotiation and request/response correlation; typed admission,
  observation, transcript, recovery, follow-up and retirement requests. These are
  internal runner contracts, not newly enabled operator RPC methods or management
  protocol changes. Later Tasks implement the reserved operations' behavior.
- Forge store revision **20**: `durable_assignment_runtimes` retains explicit backend,
  unique Assignment/runtime/conversation/store/operation identity, the full immutable
  typed launch and admission result. Launch includes Check attempt, profile revision,
  incarnation, Forge Revision and retained component identity. Full typed payload
  equality supplies conflict checking without a lossy fingerprint or secrets.
- `durable-runtime` resource reservation commits with identity. Existing capacity,
  replacement and worktree guards account for it after closure/restart. SQL guards
  prevent generic resource cleanup from releasing it. Verified durable cleanup is
  deliberately unavailable until T09, not inferred from rejection or process absence.
- Ordinary opening rejects incompatible stores before initialization writes. The
  existing explicit offline upgrade path expands 19 → 20 with empty runtime tables
  and guards, no historical identity backfill. Frozen historical fixtures and
  rollback tests cover the new target. See `crates/store/README.md` and
  `docs/state-upgrades.md` for the updated contract.

## Admission behavior

1. Authorize Workspace and original Assignment/profile/Check binding before replay.
   Exact accepted/rejected retries return the original result, independent of
   current profile selection. Conflicting payload or terminal result rejects.
2. Reserve one immutable runtime and capacity marker transactionally. New sends
   require an open provisioning Assignment, open bound attempt and current generation.
3. Only one sender transitions pending → unknown before crossing the transport
   boundary. External calls occur without a Forge writer transaction.
4. Unknown retries observe the original operation; they do not blindly resubmit.
   If intent committed but no request arrived, unknown remains explicit until
   authoritative observation resolves it. Absence is not manufactured.
5. Revalidate scope/binding when recording a response. A late result may record
   history after revocation without restoring execution authority or releasing
   resources. Runtime acceptance is never an authenticated Outcome or Assessment.

## Retained test evidence

Nine store/application tests in `durable_execution_tests.rs` cover:

- exact replay after reopen, wrong-Workspace denial, payload/result conflicts;
- remote admission followed by dropped reply, then read-only reconciliation with
  exactly one fake admission call;
- interruption after reservation, after unknown intent but before send, and after
  remote admission but before local result persistence;
- injected capacity-insert failure rolling back runtime reservation, and an actual
  SQLite commit-hook failure rolling back result acceptance to durable unknown;
- independent SQLite connections racing exact claims: one runtime and initial sender;
- conflicting concurrent identities: one winner, one structured conflict;
- closed unknown runtime retaining Project capacity and rejecting replacement/send,
  generic retirement rejection, and late accepted-result persistence;
- current profile selection not changing historical backend/binding, wrong attempt
  rejection and stale-generation send denial;
- stable rejected results, external transport execution outside the local writer
  lock, and byte-preserving incompatible schema rejection.

One upgrade test injects revision-20 stamp denial and checks complete rollback,
empty expansion, preserved inventories and idempotent explicit upgrade. Two RPC
unit tests cover strict serialization, unknown operation rejection, response
correlation and protocol/schema/component mismatch diagnostics. The existing
historical upgrade, Herdr dispatch, authenticated Assignment and capacity suites
remain green.

These are fake-transport and SQLite restart/concurrency tests, not OS process
crashes, exclusive runtime ownership, real runtime opening or physical power-loss
verification. No new dependencies were added.

## Commands and final results

Run from the Forge repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All pass on the final implementation snapshot: **577 passed, 0 failed, 1 opt-in
probe ignored**. The ignored probe is not claimed as coverage. The focused filter
`cargo test -p forge-store durable_admission` passes ten tests (nine admission tests
plus the explicit upgrade rollback test); two runner-wire tests pass in the full
suite. Early fixture/compilation/lint failures were corrected before the final gates.

Self-review against T03 and `CODING_STANDARDS.md` found no remaining findings in
this scope. This is not T18's independent completed-system review, nor T19's real
provider/package verification.

## Successor boundaries

T04 must implement actual matched transport adaptation, owner-only filesystem
layout outside Managed Worktrees, exclusive host ownership before writable open,
single-owner runtime conversation/store handling, and read-only observation. Store
IDs here are opaque reserved identities, not validated filesystem paths. Negotiation
and correlation definitions are not evidence of a running compatible component.

T05 must add full live execution authorization and bounded leases; T06 handles
profile/authentication preflight, and T07 integrates Check-bound production dispatch.
Later Tasks implement recovery, interrupted effects, verified retirement, follow-ups,
transcript pagination and packaging. No current server path calls this admission
service or launches durable work. The durable profile and bound attempt used in tests
are isolated fixtures, not production profile registration support.
