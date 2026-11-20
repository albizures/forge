# T10 — Consistent predecessor validation at recovery preparation

## Status: partial safety correction

The application-owned `Grant::validate_candidate` now validates the historical
predecessor lease with the same policy as fresh `Prepared::verify`, before the
receiving supervisor can prepare its watchdog. Previously the transition check
validated binding, sequence succession and fresh authority but did not validate
historical lease shape. A recomputed candidate commitment did not fix that omission.
The trusted coordinator already performed the stronger check; this finding does
not demonstrate an unauthorized production recovery or a bypass of the separate
protected predecessor receipt.

One shared helper rejects zero sequence, wrong launch/generation, malformed owner,
empty predecessor epoch and invalid historical lease duration. It validates history
at its original issue time, not the new owner's clock. Sequence normalization is
only for checking historical shape: a valid prior renewal need not be sequence one.
An expired predecessor and valid history from an earlier boot remain acceptable
**evidence**, not permission. No deadline, boot comparison, claim, commit or receipt
rule is weakened.

Three added Rust tests exercise:

- Recomputed commitments with 12 malformed predecessor variants; rejection remains
  terminal, no watchdog grant is accepted, and a subsequent valid preparation cannot
  revive the receiver.
- Direct transition validation and fresh preparation both reject malformed history;
  fresh preparation never reaches the external verification port in those cases.
- Valid expired sequence-six history, including a different boot with larger old
  boottime values, still permits unarmed preparation and the exact grant handshake.

The direct transition regression failed with the new shared call removed and
passed after restoration. The receiver regression also failed without the call:
zero-sequence history reached the later watchdog-construction rejection rather
than the terminal preparation-validation fence. With the shared check, rejection
occurs at the existing terminal fence. No new serialized authority is introduced.

## Verification actually run

From the repository root:

```sh
cargo test -p forge-runner-host --lib receiver_malformed_predecessor
cargo test -p forge-runner-host --lib recovery_transition_and_fresh_preparation
cargo test -p forge-runner-host --lib
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final gates pass: **797 Rust harness tests**, seven custom-harness supervised
scenarios, one pre-existing opt-in ignored; runner typecheck and **76 Node tests**
pass. The first combined verification tool call exhausted its 120-second command
budget during the workspace suite (after formatting, Clippy and host unit tests
passed). It was interrupted, not a completed workspace result. A separate complete
default-parallel workspace run with a sufficient command budget passed. No new
production timeout, test sleep or serialization was added.

No writable recovery, real-provider, packaging or power-loss verification ran.
There is no storage schema, wire version, dependency, CLI or capability change.
Only malformed history is newly rejected by the private receiving-host transition
check; use matched application/host components.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: still partial.
This correction closes a defensive preparation discrepancy; it does not implement
trusted Git/protected-proof handoff, daemon reconciliation or writable resume.
“Safe restart resumes the same attempt/conversation” remains unproved. Full restart
matrices, fresh Continuation/Review successor evidence and the domain documentation
completion remain outstanding. No unrelated behavior or completion claim was added.

Summary: T10 remains pending; missing executable writable recovery and trusted
handoff/orchestration are the worst gaps. This is self-review, not independent T18.

## Coding-standards review

Reviewed the modified application policy and host boundary tests against
`CODING_STANDARDS.md`. Policy stays in `app` (ARCH-1); external verification is
still outside transactions and malformed history fails before external I/O.
Terminal denial preserves uncertainty rather than resetting authority (SAFE-2/3).
Behavioral regressions include a demonstrated failing pre-fix transition and
negative/positive history cases (TEST-1/2), with fixture-local state and fake clocks
(TEST-3). No introduced documented standards violations found.

Production dispatch and automatic recovery remain disabled. No live Records,
configuration, credentials, runtime evidence or worktree contents were changed.
