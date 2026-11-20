# T10 increment — Forge-side recovery candidate gate

## Status and boundary

T10 remains **partial**. This adds Forge-side snapshot inspection alongside the
existing committed runtime inspector. It does not claim recovery authority,
issue/replace/renew a grant, prove process absence, open writable runtime storage
or wire automatic daemon recovery. T11 must not advance. Production durable
launch and automatic recovery remain disabled.

## Implementation

- `crates/app/src/durable_execution/recovery.rs` owns the original-Task/attempt
  policy and trusted internal `Repository::recovery_candidate` port. The returned
  candidate retains the exact Launch, accepted admission, full original attempt,
  bound execution directory and latest original grant as **evidence only**.
- `crates/store/src/durable_execution/recovery.rs` reads all facts in one SQLite
  transaction snapshot. Workspace, immutable backend/profile/attempt/runtime
  binding, live Assignment/attempt state, freshness, Task mode/lifecycle, Spec
  execution authorization and unfinished dependencies are checked before return.
- Missing/unknown/rejected admission, Dispatch Holds, interrupted-effect,
  missing-Outcome, authorization/retirement/unknown resource fences, competing
  live Assignments or closed Assignments with retained resources reject.
- The original runtime capacity reservation must remain live. An active worktree
  claim must match the persisted Assignment execution directory and Project.
  The original grant must exist and bind the same Launch/generation. Historical
  profile sources, model credentials and external components are not read.
- This is strictly a preflight snapshot: concurrent reads may both succeed. No
  claim, audit mutation, counter change, hold clearance, capability, timestamp,
  repository adoption or accepted-result replay is introduced. No schema, wire,
  dependency or existing grant-policy change is made.

A path match is not a current filesystem/Git observation. This gate neither
requires a clean checkout nor accepts dirty work as new evidence: worktree bytes
are not accessed. Actual observations, applicable retained-work rules, exclusive
host ownership, descendant safety, committed runtime input/effect inspection and
transactional recovery claims remain independent prerequisites. The returned
old lease may be expired; it identifies the predecessor, not a permissible new
owner. Existing grant issuance still rejects replacement epochs/owners and late
revival. No caller currently consumes this port to execute recovery.

## Verification

Commands run from the repository root on this increment:

```sh
cargo test -p forge-store durable_execution::tests::recovery_tests -- --nocapture
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Results:

- **9 new store tests pass.** These use isolated persistence-policy fixtures,
  not actual worktrees/host recovery. They cover every immutable Launch field,
  wrong Workspace, stale/closed/blocked authority, mode/lifecycle drift,
  missing or mismatched directory bindings, admission states, missing original
  grant/runtime, latest retained grant selection, unfinished dependencies,
  effect/missing-Outcome/authorization/unknown fences without a hold, competing
  retained resources, repeated database reopen and concurrent inspection.
- Logical snapshots of **all database tables** are unchanged after successful
  and denied inspections. A candidate does not permit a restarted daemon/new
  worker to issue a replacement grant or extend an expired original deadline.
- Formatting and Clippy pass. Default-parallel workspace tests pass:
  **714 Rust harness tests plus all 7 custom-harness supervised scenarios**;
  one pre-existing opt-in probe is ignored. Retained-work, Check-attempt and
  Continuation regressions remain green, not a new recovery-to-successor demo.
- Runner typecheck and **64 Node tests pass**, including existing real process
  death/read-only candidate and interrupted-effect preservation coverage.
  Runner source and scheduling are unchanged by this increment.
- `git diff --check` passes. No packaged-component or real-provider release
  verification ran.

During development, initial focused runs exposed fixture errors (a scoped-thread
borrow, SQLite UPSERT ambiguity, attempts to delete retained grants/retire a
runtime without proof) and Clippy flagged a test string allocation. Those were
corrected in the test code; no retention trigger or safety policy was weakened.
All checks above were rerun successfully on the corrected snapshot.

## Implementation review

Against “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
The Forge and runtime preflight ports now exist, but neither is a recovery claim
or execution permit. Transactional claim/history, host cleanup composition,
commit-before-delivery recovery leases and authorized writable reopen remain
unimplemented.

Against “Safe restart resumes the same attempt/conversation with no duplicate
admission” and “Daemon and runner restart order cannot bypass gates”: inspection
preserves original identity and does not duplicate admission, but successful
resume and the full restart-order matrix are still missing.

Against “Continuation and independent Review successors receive fresh
conversations and exact approved context”: existing regressions pass; the new
durable demonstration and glossary/ADR recovery updates remain outstanding.
No unrelated behavior or premature rollout was introduced.

Summary: T10 is incomplete; authorized writable recovery remains the principal gap.

## Coding-standards review

Reviewed the new app/store modules and tests, module wiring, and evidence changes
against `CODING_STANDARDS.md` and surrounding admission/lease/retirement contracts.
Domain attempt policy is in app; consistent SQL enforcement is in store; there
are no external calls inside transactions, new secrets, claims, lease waivers,
dependencies, schema changes or destructive operations. Tests distinguish logical
persistence fixtures from real supervised/provider evidence and use isolated
databases with a barrier for concurrent reads.

Summary: no introduced coding-standards violations found in this increment.
This is not independent T18 review or T10 completion certification.
