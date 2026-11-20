# T10 — Claim-bound quiesced runtime inspection

## Status: partial; no writable recovery or new execution owner

Combines the active original-candidate reservation, exact original-supervisor
quiescence receipt and pinned read-only pending-input/effect inspector under one
retained exclusive lock. This is a concrete recovery-host **component**, not a
`RecoveryHost` implementation. No scheduler, grant delivery, waiting-owner handoff,
writable Harness reopen or new admission invokes it. T10 remains pending.

## Delivered boundaries

- App exposes `claimed_recovery_candidate`, distinct from historical idempotent
  `claim_recovery`. Store reads scope, exactly one active matching claim, live
  attempt/dependency/parent/resource gates and the complete candidate commitment
  from one transaction snapshot. Cancelled/consumed/missing claims, changed
  context and foreign operations cannot recover permission through claim replay.
- Server `quiesced_inspection::inspect` derives the canonical runtime mapping,
  invokes the original-admission inspector and retains its exclusive lock. It then
  requires the exact protected original-host intent/receipt and rechecks the live
  claimed candidate. There is no release/reacquire gap between runtime and process
  proof acceptance. The original inspector rejects settled input and committed
  interrupted effects. The guard's `verify` rechecks mapping, protected storage,
  original receipt and current claimed state without accepting a historical audit.
- Missing profile sources/model credentials do not enter this historical read.
  No runtime/worktree bytes, claim timestamps, attempt counters, evidence, grants,
  resources or capacity are changed. Dirty bytes are preserved, not validated or
  adopted as completion evidence. Git freshness/retained-work policy remains a
  separate outstanding verification obligation.

## Inspection lock lifetime correction

The first full workspace run hit the pre-existing immediate lock-reacquisition
failure, this time in
`recovery_inspection_rejects_foreign_scope_bindings_components_effects_and_done_input`.
A deterministic regression duplicates the guard's open file description, drops
its original guard, then attempts another exclusive lock. This fails without the
fix and passes with it. Linux flock survives closing one descriptor while another
reference exists; CLOEXEC does not prevent a concurrent fork temporarily retaining
that reference before exec. The test demonstrates this kernel lifetime mechanism,
not a traced proof of which process caused each historical intermittent failure.

`ExclusiveInspection::drop` now explicitly unlocks before closing. Unlock failure
emits a static diagnostic and falls back to close; any surviving reference remains
fail-closed. This changes only read-only inspection lifetime: supervisors/workers
still deliberately share and retain their execution flock. No timeout, retry sleep,
suite serialization or process-adoption permission was added. The final full
workspace rerun passed. Historical unrelated supervisor timeouts remain undiagnosed.

## Verification

Actually run from the repository root:

```sh
cargo test -p forge-store claimed_candidate
cargo test -p forge-runner-host --test authorization_session quiesced_inspection
cargo test -p forge-runner-host --lib inspection_drop_releases_flock
cargo test -p forge-runner-host --lib inspection
cargo test -p forge-runner-host --test authorization_session recovery_inspection
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
```

The new duplicate-description regression was run and failed before the fix, then
passed after it. Final formatting, Clippy and default-parallel workspace gates
passed: **778 Rust harness tests**, seven supervised scenarios, one existing opt-in
ignored. Runner typecheck and **76 Node tests** passed. Initial new integration
tests had Rust lifetime/match-arm compile errors; corrected. Initial Clippy found
unnecessary raw-string hashes; corrected without suppression.

Nine new Rust tests cover:

1. Active exact claim reads across store reopen with every table unchanged.
2. Live-state/context/worktree/effect/hold/closure drift despite accepted historical
   claim replay; retired/deleted claims never reconstruct authority.
3. Malformed/duplicate claim history rejection without repair.
4. Actual committed claim/private quiescence/subreaper receipt plus pinned SQLite
   inspection for queued and placed input; exclusive contention, dirty/runtime/
   owner/resource/capacity preservation and repeated read-only observation.
5. Missing/corrupt/unsafe receipt and conflicting intent rejection without repair.
6. Settled input and interrupted effects block despite real process-cleanup proof.
7. Receipt loss, claim consumption and changed bound context **during external
   inspector I/O** reject an otherwise valid matched runtime reply.
8. Retained guard rechecks closure, claim cancellation and protected receipt loss.
9. Explicit inspection unlock with a retained duplicate open description.

The actual processes are controlled synthetic workers with isolated databases and
real Linux locks/subreaper receipts, not a real-provider recovery demonstration.
No model/tool is resumed by the new adapter; no physical-power-loss or packaged
release verification was run.

## Compatibility

No schema, dependency, runtime/wire, CLI/RPC or Management Protocol change. The new
app port and server adapter are internal only. Existing original-host receipts and
claim history remain unchanged. No live Records, configuration or credentials changed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
This increment connects previously separate read-only proofs, not the full trusted
host composition. Protected waiting-owner handoff, external Git policy, retained
proof digest, executable recovery delivery, daemon wiring and writable resume are
still absent. “Safe restart resumes the same attempt/conversation” and the fresh
Continuation/Review successor demonstration remain unfulfilled. No scope creep or
incorrect execution-authority claim found.

Summary: T10 remains incomplete; authorized writable recovery is the main gap.
This self-review does not replace independent T18 review.

## Coding-standards review

Reviewed all introduced app/store/server/host/tests against `CODING_STANDARDS.md`.
App owns the live-read port; store validates a consistent transactional snapshot;
external proof stays outside database transactions with post-I/O revalidation.
Failures preserve evidence/capacity and do not repair paths or release resources.
The lock correction explicitly handles release failure with a static diagnostic.
Tests exercise kernel/runtime/process boundaries without sleeps or serialization.
No introduced standards violations found; this is not T10/release certification.
