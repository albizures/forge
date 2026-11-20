# T10 — Recovery candidate inspection increments

## Status: partial; T10 remains pending

The original runtime-inspection increment is recorded below. The subsequent
[Forge-side candidate gate](forge-candidate-gate.md) adds consistent read-only
application/store binding inspection and nine persistence-policy tests; all four
repository gates, 714 Rust harness tests plus seven supervised scenarios, runner
typecheck and 64 Node tests pass. Neither increment authorizes execution. Claims,
recovery leases, daemon composition and writable reopen remain unimplemented.

The original increment implements only committed runtime candidate inspection. It does
**not** deliver authorized writable recovery, change lease issuance or enable
automatic daemon recovery. T11 must not advance on this evidence. Production
durable dispatch remains disabled. No Forge Records, operator configuration,
capabilities or live worktrees were changed.

## Implemented

`runner/src/recovery.ts::inspectRecoveryCandidate(directory, launch, admissionId)`:

- Snapshots the full immutable Launch before awaiting and validates protected
  runtime storage boundaries.
- Requires existing initialization and the exact journal Launch; never creates,
  resets or repairs an absent/partial store.
- Reuses the committed whole-store interrupted-effect inspector, including its
  valid unowned root check, before returning any candidate.
- Requires the original request's exact accepted admission, root conversation
  and `input` type. Only `queued`/`placed` inputs qualify as candidates.
- Rejects finished and failed inputs as intervention states. It never submits
  another input or `continue`, manufactures a Continuation, opens a Harness,
  creates a model/tool or schedules execution.
- Returns only bounded identity/status evidence, not prompt/tool contents.

The result is a **necessary runtime precondition**, not execution authority or a
proof of process absence. A future controller must hold exclusive ownership
through inspection and authorized open. Forge authority, freshness, bindings,
retained-work rules, leases and host verification remain independent gates.
Existing restarted issuers still fail against retained grant history.

No dependency, wire contract, Forge schema or runtime schema change is made.
The helper is not wired into the daemon, supervisor or writable runner path.

## Reproducible verification

From the repository root:

```sh
(cd runner && node --test test/recovery.test.mjs test/interrupted-effects.test.mjs)
(cd runner && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Observed results:

- Runner typecheck and **64 Node tests passed** on the final code snapshot.
  Ten new recovery tests cover absent/partial initialization, a valid root with
  missing admission, queued/placed/done/unanswered input, passive/continue input,
  all immutable Launch fields, mismatched admission and caller mutation.
- `recovery-worker.mjs` opens the actual pinned Harness/SQLite runtime with a faux
  model. The parent waits for the original input/admission and model-entry
  signals, kills the Node process with SIGKILL and waits for exit. Three repeated
  read-only inspections find the same placed input/root/admission with unchanged
  database and nonempty WAL bytes. There is one model-entry event and no reopen
  or second submission. This is real process death, **not supervised recovery**.
- The existing real guarded Bash/publication-like external-mutation test now
  invokes the recovery candidate gate after the effect occurred but before its
  result committed. It rejects the original accepted admission on each repeated
  inspection, with one external mutation and unchanged Git/runtime evidence.
- Rust formatting and Clippy passed. Full default-parallel workspace tests
  passed: **705 Rust harness tests plus 7 supervised tracer scenarios**, with
  one pre-existing opt-in test ignored. This includes retained-work, Check
  attempts and Continuation regressions; it does not demonstrate a new durable
  recovery-to-successor flow.
- The initial chained Rust check command exceeded its 120-second command timeout
  during workspace tests. A complete rerun with a larger deadline passed. No
  product fix or diagnosis of historical transient failures is claimed.
- `git diff --check` passed. No packaged-component or real-provider test ran.

SQLite read-only handles can create empty WAL/SHM coordination files; byte
assertions exclude SHM and empty WAL, not committed runtime history. No physical
power-loss durability or hostile same-user containment is inferred.

## Remaining T10 acceptance work

1. Add application/store-owned recovery authorization and transactional claims:
   current Assignment/attempt/profile/generation, evidence/freshness and
   retained-work validation; revocation/expiry/effect/retirement/hold rejection;
   concurrent claims, immutable history and lost-commit/reply behavior. Preserve
   original admission, capacity and all attempt/evidence identities.
2. Compose canonical ownership and process safety with daemon reconciliation and
   fresh commit-before-delivery recovery leases. A retained flock or descriptive
   PID is not descendant cleanup proof; do not adopt old grants or controllers.
3. Implement an authorized writable reopen/resume path that cannot re-admit the
   original input, replay arbitrary effects or silently repair/adopt dirty work.
4. Exercise fake-clock restart order/ownership/freshness/revocation matrices and
   actual owned supervisor death followed by successful same-attempt recovery.
5. Demonstrate accepted Implementation Continuation and independent Review
   successors with fresh conversations and exact approved context, only after
   retirement/reconciliation. Missing Outcomes must still block successors.
6. Apply Assignment Execution Recovery in `crates/CONTEXT.md` and complete the
   recovery portion of the actual recovery/retirement ADR (T09 identifies actual
   ADR-0012; do not assume the Spec's provisional number). Update operations docs.

## Implementation review

Against T10's “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
Only committed runtime input/effect inspection is implemented; no controller or
writable recovery path consumes it yet.

Against “Safe restart resumes the same attempt/conversation with no duplicate
admission” and “Daemon and runner restart order cannot bypass gates”: successful
resume and the complete restart matrix remain unimplemented. The new crash test
proves read-only preservation/candidate inspection only.

Against “Continuation and independent Review successors receive fresh
conversations and exact approved context”: existing regressions pass, but the
new durable recovery/successor demonstration is missing. Glossary/ADR recovery
updates also remain outstanding. No scope creep or incorrect execution-authority
claim was found in this increment.

Summary: T10 is incomplete; absence of authorized writable recovery is the main
acceptance gap. Do not mark complete or enable scheduling.

## Coding-standards review

Reviewed this increment's runner source/tests and documentation against
`CODING_STANDARDS.md`, including surrounding read-only storage/effect contracts.
It adds no Rust/store policy, secrets, dependency, destructive worktree operation,
lease waiver or mutable API contract. Tests distinguish storage fixtures from
real process/effect evidence and use isolated directories and event-based crash
synchronization. Existing user-owned planning changes were preserved.

Summary: no introduced coding-standards violations found in this increment.
This review does not certify T10 completion or replace T18.
