# T10 — Private recovery delivery transport

## Status: partial; sender adapter and synthetic consumer composition

`ControlPipe` now implements the recovery coordinator's specialized `Delivery`
port. It writes a strict, separately versioned `SupervisorRecovery` record with
`host_recovery: 1` and the complete committed claim/lease/evidence envelope. This
is not an ordinary lease with a relaxed initial sequence. Delivery remains one
bounded nonblocking write to the directly spawned child's taken stdin, with no
path/PID/file-based reconstruction, flush or local byte retry.

All control kinds now share a terminal framing fence across `ControlPipe` clones.
A partial write, zero write, interruption, full/broken pipe, serialization failure
or oversized record prevents subsequent writes, including another control kind.
Previously the application coordinator stopped on delivery failure, but a cloned
pipe itself could append another JSON record to an incomplete one. The channel
now independently preserves that uncertainty. Retirement cannot bypass a failed
channel; retained intent/capacity and separate reconciliation remain necessary.
Successful writes are transport results, not consumer acceptance or cleanup proof.

The executable supervisor and production runner do **not** initialize recovery
consumers or accept this new handshake yet. No daemon invokes this sender for
recovery. Tests directly compose the sender with a synthetic Node consumer using
fixed expected identity and a fake clock, not trusted predecessor verification.
No actual runtime store, Harness, model or tool is opened by the new fixture.
T10 remains pending; production and automatic recovery remain disabled.

## Verification

Commands run from the repository root:

```sh
cargo test -p forge-runner-host control
cargo test -p forge-runner-host --test recovery_control
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final results: **760 Rust harness tests passed**, seven custom-harness supervised
scenarios passed, one pre-existing opt-in test ignored. Runner typecheck and
**76 Node tests** passed. Formatting, Clippy and diff whitespace checks passed.
Eight new Rust tests cover:

- Strict nested envelope decoding, exact round trip, separation from ordinary
  control, and acceptance by the existing application watchdog policy.
- One-write partial/zero/interrupted/blocked/broken transport outcomes, terminal
  refusal of later recovery/ordinary/retirement records, bounded rejection before
  any write, and separate newline framing for recovery followed by renewal.
- Actual `ChildStdin` delivery to the pinned Node `Authorization` consumer:
  unchanged exact envelope, sequential ordinary renewal through a clone, stale
  recovery denial after renewal, mismatched evidence, ordinary startup denial,
  and oversized-record fencing shared across actual pipe clones.

The real Node subprocess does not include the executable Rust watchdog, SQLite
commit coordinator, predecessor cleanup or ownership handoff. Earlier tests cover
those existing components separately; no complete recovery composition is claimed.
No new packaged, real-provider or physical-power-loss verification ran. No live
Assignment, worktree, credential or operator configuration changed.

Initial Clippy rejected missing Markdown backticks around `ChildStdin` in a new
test comment. That comment was corrected before the complete passing gates. No
historical transient test failure is claimed fixed.

## Compatibility

Additive, separately versioned internal private control only. Ordinary grant,
retirement, operator CLI/RPC, Management Protocol, dependencies and database
schemas are unchanged. Matched recovery receivers remain unimplemented; adding a
sender is not permission to send the new control to ordinary startup. A future
host must freshly prepare the independent watchdog and Node consumer from trusted
verification, then use commit-before-delivery while retaining exclusive ownership.
Never initialize their expectations from an audit or an operator/agent payload.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates.” This increment
implements only the private sender port and transport framing fence. Trusted host
verification, protected ownership handoff, executable recovery reception, daemon
reconciliation and writable reopen remain missing.

“Safe restart resumes the same attempt/conversation with no duplicate admission”
and the full restart-order matrix remain undemonstrated. The synthetic consumer
never opens a conversation. Fresh Continuation/Review successor demonstrations
and glossary/ADR recovery completion also remain outstanding.

No scope creep or incorrect execution-authority claim found. Summary: T10 is
partial; the worst gap remains safe authorized writable recovery. This is
self-review, not independent T18.

## Coding-standards review

Reviewed the changed RPC contract, private sender, tests and surrounding lease/
recovery ports against `CODING_STANDARDS.md`. Policy remains in app, wire in rpc,
and process transport behind the host adapter. The sender issues no authority,
performs no store mutation and logs no payloads. Shared terminal fencing preserves
partial-write uncertainty across clones and control kinds. Tests isolate children,
use explicit events and distinguish real transport from synthetic host proof.
No dependency, secret exposure, destructive operation or silent compatibility
fallback is introduced.

Summary: no introduced coding-standards violations found. This is not a T10 or
release-readiness certification.
