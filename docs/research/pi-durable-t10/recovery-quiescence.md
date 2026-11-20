# T10 — Claim-bound predecessor quiescence

## Status: partial; real owned shutdown proof, not writable recovery

Adds a separate internal stop operation for a reserved same-Assignment recovery.
Unlike retirement it does not close/revoke the Assignment, retire resources,
consume its recovery claim, release capacity or manufacture a Check attempt. The
new real-process tests persist an actual SQLite recovery claim, deliver through
the original supervisor's private pipe and inspect its protected receipt.

This is a prerequisite for the future trusted recovery host, not that host itself.
No daemon reconciler, operator CLI/RPC, `RecoveryHost`, waiting-owner handoff or
writable reopen invokes it. T10 remains pending; T11 and production/automatic
recovery remain blocked. Loss of the original supervisor or previously uncertain
process identity still blocks this path; no PID adoption or dead-host proof is
introduced. Successful cooperative shutdown is not a crash-recovery demonstration.

## Mechanism and boundaries

- App owns the distinct `recovery::quiescence::{Operation, Receipt}` policy. The
  operation binds the full reserved candidate digest/claim and exact original
  lease. Construction rejects a different candidate, launch, generation or daemon
  epoch. The old lease is historical stop evidence, not renewable authority.
- RPC defines strict, separately versioned `SupervisorQuiescence` with
  `host_quiesce: 1`. `ControlPipe::quiesce` uses the existing bounded nonblocking
  one-write framing fence shared by all pipe clones and control kinds. The caller
  must have persisted the claim; private transport is trusted, not an agent API.
- The executable watchdog requires the exact last accepted predecessor grant,
  original owner, no retained startup blocker and no earlier host fault. It
  fences lease acceptance and dispatches shutdown without forwarding a cancellation
  payload to the worker. Conflicting retirement/quiescence requests permanently
  deny further attempts and do not replace the first shutdown identity.
- The existing direct-child subreaper mechanism is shared with retirement without
  sharing its intent/receipt names or revocation policy. It verifies original
  storage/ownership, durably writes `recovery-quiescence-intent.json` before TERM,
  performs at most one verified group escalation, and accepts only kernel ECHILD
  plus unchanged protected storage as descendant-absence proof. Leader exit or
  successful signals alone are insufficient. A surviving escaped child prevents
  a receipt even after the worker is killed.
- Only then does the original host persist
  `recovery-quiescence-receipt.json` with the exact operation and exit, releasing
  its flock. A failed intent/receipt write leaves the host holding ownership;
  existing evidence is never rewritten or repaired. Failed shutdown has no
  fallback watchdog signals or local retry path.
- The protected reader checks bounded strict receipt/intent, exact operation and
  canonical owner-only storage/lock identity. Callers must separately retain
  exclusive inspection through dependent verification/commit. Inspection neither
  signals nor modifies runtime/worktree bytes. The retained intent is also an
  ordinary-supervisor startup fence, including after a successful receipt.

The watchdog loop rechecks pending shutdown under its policy mutex before polling
lease expiry, so a request arriving after the initial loop snapshot is not mistaken
for an expiry-triggered signal ahead of protected intent. Original retirement
continues to require revocation in its existing application/store path.

This proof covers the original dedicated supervisor's descendants on the trusted
local host. It does not prove external effects safe, fence arbitrary malicious
same-user processes, clear holds/resources or authorize a replacement executor.
Fresh runtime input/effect inspection, repository freshness/retained-work checks,
exclusive ownership handoff and transactional fresh grant remain independent gates.

## Verification

Commands actually run from the repository root:

```sh
cargo test -p forge-app shutdown_binding
cargo test -p forge-runner-host quiescence
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final default-parallel workspace rerun: **769 Rust harness tests passed**, seven
custom-harness supervised scenarios passed, one pre-existing opt-in ignored.
Runner typecheck and **76 Node tests** passed. Formatting, Clippy and whitespace
gates passed. Nine new Rust tests cover:

1. Original-bound stop representation, epoch/grant shape and changed-candidate
   rejection; no old deadline extension.
2. Strict nested wire decoding, round trip and separation from retirement/recovery
   grant controls.
3. Watchdog exact last-grant requirement, unarmed denial, exact pending retries,
   first-identity retention and terminal conflicting-claim denial.
4. Stale grants, previous faults, retained markers, owner drift and cross-kind
   shutdown conflicts.
5. Actual committed claim/private pipe/original subreaper shutdown with a normal
   child and delayed cooperative worker exit. The Assignment/attempt/resource
   state, capacity, original owner evidence, dirty work and runtime sentinel bytes
   remain unchanged. Repeated receipt reads do not grant authority; old renewal
   and an otherwise valid direct sequence-one control after restart are denied.
6. Foreign predecessor identity causes no signal/intent/receipt and retains flock.
7. Actual escaped child survives group shutdown; incomplete proof retains the
   claim, live Assignment, capacity and flock without a receipt or signal retry.
8. Conflicting protected intent or receipt is never repaired or converted into
   cleanup success, including receipt failure after descendants really exited.
9. Missing/foreign/malformed/oversized/false receipt and mismatched intent rejection.

Process tests use an existing controlled Node worker and isolated store/worktree
fixtures, not a real provider or production dispatch. The runtime preservation
sentinel is deliberately not a SQLite recovery test. The suite's existing runtime,
retained-work, Continuation and Check-attempt tests also pass, but no successful
same-conversation resume or fresh successor cycle is inferred.

An initial new test placed its runtime sentinel before the first grant; existing
retained-runtime preflight correctly denied that grant. The sentinel was moved
after claim setup. Initial Clippy found a long watchdog function and test comment/
import/naming issues; shutdown variant dispatch was extracted and the test issues
were corrected without lint suppression.

One later full workspace run failed the **existing**
`canonical_recovery_inspection_preserves_original_input_and_holds_exclusive_lock`
assertion reacquiring a dropped inspection guard. That target and a complete
unchanged default-parallel rerun passed. This recurrence is retained as an
unresolved intermittent failure, not diagnosed, waived by serialization or claimed
fixed. An earlier complete run before the final added wire test also passed.
No new packaged, real-provider or physical-power-loss verification ran.

## Compatibility

No database/runtime schema, dependency, ordinary lease, retirement, operator
CLI/RPC or Management Protocol change. The private stop control is additive and
separately versioned; protected recovery intent/receipt artifacts have distinct
names and version-one contents. Old hosts do not implement this control; only
matched components may receive it. No automatic marker migration/removal is added.
The recovery-grant receiver is still absent, and quiescence does not enable it.
No live Assignment, operator configuration, credentials or worktrees changed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates.” This increment
adds actual owned predecessor shutdown proof only. A concrete recovery host still
must combine that proof with exclusive runtime/effect inspection, external policy,
protected handoff and committed grant delivery. Daemon integration and writable
reopen remain missing.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
is not demonstrated. Original-supervisor death and prior host faults remain
blocked; full restart-order matrices, fresh Continuation/Review successors and
recovery glossary/ADR completion are still outstanding.

No scope creep or incorrect execution-authority claim found. Summary: T10 remains
partial; safe writable resume and its complete trusted composition are the worst
acceptance gaps. This is self-review, not independent T18.

## Coding-standards review

Reviewed the new app policy, strict private wire, host mechanism, watchdog dispatch,
startup marker guard, tests and operations docs against `CODING_STANDARDS.md` and
the surrounding lease/retirement boundaries. Policy remains in app; transport,
filesystem and process mechanics remain behind host interfaces. There is no
external I/O in a store transaction, generic claim mutation, secret logging,
worktree repair or process adoption. Protected intent precedes signals; original
identity and evidence survive failure; proof does not release capacity. Tests use
isolated fixtures, event synchronization and bounded real failure cases, and retain
the observed intermittent failure truthfully.

Summary: no introduced coding-standards violations found. This is not a T10 or
release-readiness certification.
