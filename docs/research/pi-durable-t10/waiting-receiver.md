# T10 — Waiting-owner inspection and executable recovery receiver

## Status: partial; committed reception, not writable recovery

Builds on [quiesced inspection](quiesced-inspection.md). A directly spawned waiting
supervisor can now be freshly inspected, prepared without authority, and armed
through the exact committed recovery envelope. The positive tracer uses the actual
SQLite recovery transaction and application coordinator, actual Linux supervisor
and actual `runner/src/main.ts`. It never opens a writable Harness, resubmits the
original input, bootstraps another Check or resumes a model/tool.

The external `RecoveryHost` verification/digest in that tracer is **explicitly
synthetic**. No complete Git/freshness/protected-proof verifier, retained ownership
handoff orchestration, daemon reconciler or writable resume is delivered here.
Original-supervisor loss/prior host uncertainty still block recovery. T10 remains
pending and production/automatic recovery remain disabled.

## New-owner proof

`waiting_recovery_inspection::inspect` requires the original active content-bound
claim, exact protected predecessor intent/receipt and a **directly spawned Child**.
It verifies both host/worker shared flocks, process ancestry/start identities,
canonical storage and boot/time namespace before and after bounded original-input/
effect inspection. It revalidates current claimed Forge state after external I/O.
No PID or old owner file can construct the new live owner observation.

Spawning does not transfer a prior read-only observation: after the new supervisor
acquires its own lock, all proof is freshly inspected under that ownership. A race
loser cannot inspect or execute. The new owner metadata intentionally differs from
the predecessor; runtime/worktree/history and original receipt stay unchanged.
Dropping the daemon verifier does not terminate the waiting host or release its
flock. Receipt/owner/clock/authority failure terminally fences the verifier, even if
someone restores the file afterward.

Its explicit pre-commit `observe(repository, workspace)` rechecks the active claim.
Its `AuthorityHost` implementation checks **physical ownership/proof only** across
commit: rechecking an active claim after the grant transaction would incorrectly
reject the just-consumed reservation. The coordinator/store still revalidate live
execution authority on every initial grant/renewal. Host-fault and retirement
markers block both waiting and quiesced inspection; an old clean receipt cannot
waive a later safety fence.

## Preparation versus delivery

- RPC adds separate strict private `SupervisorRecoveryPreparation` v1 containing
  the original candidate and expected grant. This is not an operator/agent API,
  a commit receipt, grant delivery or serialized `Prepared` authority.
- `ControlPipe::prepare_recovery` accepts only opaque `Prepared` from fresh trusted
  verification, using the existing terminal one-write/nonblocking framing fence.
  Success is transport only. Callers must observe the matched worker's successful
  preparation response before committing/delivering authority. There is no generic
  raw-payload preparation method or retained-file import.
- The executable supervisor checks the exact original receipt, claim commitment,
  app-owned transition policy, new waiting owner, version, clock and absence of
  prior fault/shutdown/grant. Ownership observation faults are retained even
  before the first grant, so restored files cannot revive an unarmed host. It
  prepares the existing exact-envelope watchdog
  policy without accepting a lease. Exact preparation retries do not reset policy,
  clock observations or deadlines; conflicting or post-grant preparation fences.
- `Grant::validate_candidate` shares the existing application transition policy
  with `Prepared::validate`; the host does not independently redefine recovery
  authority or turn wire shape validation into a commit assertion.
- The actual Node entrypoint independently inspects the original pending admission
  and whole-store effects read-only, checks cwd/binding/current owner and prepares
  `Authorization.forRecovery`. Rejection terminally fences Node authorization.
  Its `recovery-prepared` response contains no context, proof payload or secrets.
- Separate `SupervisorRecovery` delivery rechecks protected predecessor proof and
  invokes the exact expected watchdog/Node handshake. An ordinary continued-
  sequence control cannot arm a waiting recovery consumer. Exact live retries
  cannot extend deadlines. Ordinary sequential renewal works only after the exact
  first envelope; old envelopes cannot reset an advanced/fenced consumer.
- No parser branch opens storage writable or interprets a recovery grant as a
  fresh input. Existing admit retries remain read-only against their journal.
  Missing Outcomes still do not produce completion or successors.

The private pipe is trusted daemon authority, not cryptographic proof that a
transaction committed. The application coordinator is the commit-before-delivery
boundary. This tracer's positive case actually uses it and checks one new lease,
unchanged attempt and unchanged pending runtime bytes. Deliberately invalid
negative wire tests do not claim a committed grant.

## Verification

Actually run from repository root (plus the predecessor commands):

```sh
cargo test -p forge-runner-host --test authorization_session waiting_inspection
cargo test -p forge-runner-host --test authorization_session executable_
cargo test -p forge-rpc recovery_preparation
cargo test -p forge-runner-host --lib receiver_
cargo test -p forge-runner-host --test authorization_session intent_commit_failure
cargo test -p forge-runner-host --test authorization_session executable_unarmed_owner_fault
cargo test -p forge-runner-host --test authority
cargo build -p forge-runner-host
(cd runner && node --test --test-name-pattern='foreign/malformed controls' test/process.test.mjs)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final formatting, Clippy, default-parallel workspace and Node gates pass:
**794 Rust harness tests**, seven supervised scenarios, one pre-existing opt-in
ignored; runner typecheck and **76 Node tests** pass. Together with the preceding
increment, 25 new Rust tests were added from the 769-test baseline. This increment
adds 16:

1–4. Real new waiting owner with queued/placed input, actual original subreaper
proof, kernel contention and unchanged runtime/attempt/resources; done/effect and
missing-component denial; lost/restored receipt/owner/host-fault fencing; wrong
proof and changed context denial.

5–11. Actual executable supervisor/Node preparation, actual store/coordinator grant
commit and exact-envelope reception/retry; revocation after preparation before
commit produces no grant; unprepared/ordinary-first denial; wrong receipt/owner/
commitment/version rejection and payload redaction; independent Node done/effect
rejection; proof loss/conflicting envelope before delivery denial; an observed
unarmed-owner identity fault remains fenced after file restoration.

12. Strict preparation wire round trip, nested unknown-field rejection and
separation from ordinary/recovery grant controls.

13–16. Fake-clock host adapter preparation retry without authority, post-grant reset
denial, ordinary renewal only after exact recovery handshake, first-expectation
retention and fault/shutdown/owner/backward-clock/exact-expiry fences.

Initial new tests had a Rust fixture-borrow error; corrected by reading the lease
count through the independently borrowed database path. Initial Clippy found three
unit-test formatting/assertion issues; corrected without suppression.

### Verification findings corrected

One Node run timed out in the existing malformed-control test: the input-router
refactor had changed a malformed ordinary lease's retained diagnostic from
`runner-authorization-denied` to the generic input fault. The specific existing
ordinary-authorization diagnostic and retirement/quiescence version diagnostics
were restored. Its focused regression and the full 76-test suite pass.

One subsequent full Rust run failed the existing retirement intent-commit failure
test's assertion that no watchdog marker existed. Its temporary Application owned
the sole controller Arc; dropping it after the injected database rejection could
close the original private pipe, independently cause worker EOF/exit and create
legitimate watchdog uncertainty before the assertion. The test now retains its
original controller through the assertions. Its target and the complete default-
parallel suite pass. No sleep, timeout extension, serialization or weaker production
retirement/expiry rule was introduced. This does not attribute or resolve every
historical supervisor timeout.

Review also found that the independent watchdog retained failed ownership
observations only after a lease existed. An unarmed recovery host could therefore
have a terminally fenced process handle without recording its fault before first
authorization. It now retains/fences ownership uncertainty even while unarmed.
The real-process regression was run against the old condition and failed waiting
for the missing diagnostic, then passed after the correction; restored owner.json
cannot permit preparation and no lease/input is admitted. One full run then exposed
an existing authority-test fixture's assumption that worker-exit was the first
diagnostic. It now waits for that same required exit event within the original
single ten-second deadline, allowing the preceding new uncertainty diagnostic.
The target and full default-parallel suite pass; no exit assertion was removed.

No real-provider, packaged-component, power-loss or writable-recovery demo ran.

## Compatibility and operational boundary

No database/runtime schema or dependency change. The additive private preparation
control has its own version; old hosts/workers do not implement it. Use matched
components; main.ts changes also change the existing component source hash. No
operator CLI/RPC, Assignment capability or Management Protocol surface changed.
No live Records, daemon configuration or credentials were changed.

Inspection/authorization does not adopt dirty progress as accepted evidence,
repair runtime receipts or clean/commit/reset a checkout. A complete controller
must still validate applicable external Git/retained-work policy and persist
protected operation-bound proof before constructing real `Prepared` authority.

## Implementation review

T10 requires “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
Actual new-owner proof and executable committed grant reception now compose in a
controlled tracer, but external Git/protected proof verification is synthetic and
no daemon/reopen path uses it. “Safe restart resumes the same attempt/conversation”
is not demonstrated: runtime bytes remain read-only. Full restart-order matrices,
accepted Continuation/independent Review successors and glossary/ADR completion
remain outstanding. No scope creep or incorrect recovery-completion claim found.

Summary: T10 remains incomplete; real trusted composition and writable same-attempt
resume are the worst gaps. Self-review, not independent T18.

## Coding-standards review

Reviewed the changed app policy, store read port, private wire/sender/receiver,
server ownership adapters, Node entrypoint and tests against CODING_STANDARDS.md.
Policy remains app-owned; store snapshots/transactions retain authority; process/
filesystem reads stay outside transactions and watchdog mutex I/O. Unknown results
retain capacity; no PID adoption, runtime repair, local deadline extension or secret
logging was added. Tests distinguish synthetic external verification from actual
store/process/runtime transport and retain observed failures/corrections above.

No introduced documented standards violations found. This is not T10/release
certification or independent review.
