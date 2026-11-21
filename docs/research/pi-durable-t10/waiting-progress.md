# T10 — Combined waiting-owner and repository-progress inspection

## Status: read-only composition, not recovery authority

`waiting_recovery_inspection::Inspection::inspect_progress` now consumes the
fresh waiting-owner inspector into one retained combined guard. The direct Child
borrow remains held; the verified supervisor/worker retain their kernel lock.
No daemon descriptor is substituted for executable ownership.

The guard binds the canonical claimed Git snapshot to the **same Candidate** as
the process/runtime inspector. Initial construction and subsequent verification:

1. Check directly spawned ownership, protected predecessor receipt, canonical
   runtime mapping and the active original claim.
2. Revalidate the retained Git binding and content/index observation.
3. Perform another bounded, read-only Node scan of committed original pending
   input and whole-store effect intent; require the identical input identity/status.
4. Revalidate Git again, followed by live claim/receipt/owner and monotonic time.

Failure terminally fences this combined object, including temporary loss later
restored. An async verification sets its fence before external I/O, so cancellation
cannot silently leave the object reusable. There is no recovery-by-repair,
adoption of dirty progress, accepted evidence update, grant or scheduling.

The combined type deliberately does **not** implement `AuthorityHost` or
`RecoveryHost`. Its claimed reads are pre-grant only: the first grant transaction
consumes that claim. Protected combined-proof persistence, exact consumer
preparation and post-consumption ownership handoff must bridge that boundary
explicitly; retaining this read-only object is not permission to skip them.
Existing receiver tracers still use synthetic external verification.

This is trusted-local-host observation, not hostile same-user containment or
universal process-absence proof. The waiting worker must remain ungranted. External
writers can still race or change-and-restore bytes between observations; the
combined guard is not terminal authorization. Existing Git subprocess/file-size
budget limitations remain. Unknown original-supervisor safety stays blocked.
No schema, wire, dependency, CLI, provider-authentication or rollout change.

## Actual verification

```sh
cargo test -p forge-runner-host --test authorization_session recovery_progress
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p forge-server --lib registered_grilling_receives_protected_capability_from_production_adapter
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final default-parallel workspace run: **811 Rust harness tests passed**, seven
custom-harness supervised scenarios passed, one pre-existing opt-in ignored.
Runner typecheck and **76 Node tests passed**. Format, clippy and diff checks pass.

Two new integration tests compose actual SQLite authority, local Git, original
subreaper quiescence proof, a directly spawned waiting owner and pinned Node
read-only runtime inspection. Original launch/admission fixtures and model work
remain controlled/synthetic, not a production crash-to-writable-resume demo.

- Dirty tracked/untracked Implementation bytes remain unchanged and distinct
  from bound Check evidence. Runtime database/WAL bytes, original attempt,
  Assignment state and capacity remain unchanged. Dropping the combined guard
  neither terminates the waiting child nor releases its executable lock.
- Changed Git bytes, missing predecessor receipt, revocation, invalid watchdog
  evidence and unavailable runtime database reject verification. Restoring Git,
  receipt, watchdog or runtime files cannot revive that verifier. Capacity remains
  reserved. The test restores only test-owned fault injections; production does
  not perform those repairs.

Initial compilation exposed a test match-arm result mismatch. First execution
exposed a fixture `.forge` permission mismatch: creating the real Managed Worktree
before runtime preparation had created mode 0755 ancestors. The fixture now
explicitly creates the protected 0700 boundary; production checks were unchanged.
Clippy required documentation backticks/format syntax and extraction of focused
fixture helpers rather than suppressing its function-size lint.

The first full workspace run failed the existing Herdr copied-executable test
with `ExecutableFileBusy`. Its focused target and a complete unchanged,
default-parallel workspace rerun passed. No Herdr fix or diagnosed root cause is
claimed. No real-provider, packaging, power-loss or writable-recovery verification
ran.

## Implementation review

T10 requires “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates.” This increment
composes previously separate process/runtime and repository observations, but
protected trusted proof/handoff, daemon reconciliation and writable reopen remain
missing. “Safe restart resumes the same attempt/conversation” remains unproved;
Continuation/independent Review successor evidence and glossary/ADR completion
remain outstanding. No unrelated feature was introduced.

Summary: T10 remains pending; the worst gaps remain trusted handoff and authorized
writable resume. This is a self-review, not independent T18 verification.

## Coding-standards review

Reviewed the new combined guard/tests and changed waiting-owner/fixture code
against `CODING_STANDARDS.md`. The server composes existing app policy ports rather
than redefining repository or authority policy (ARCH-1/2). External observations
remain outside transactions and revalidate live claims (TX-4). Errors preserve
capacity, original attempts and worktree/runtime bytes (SAFE-2/3/4); no cleanup or
permission follows a successful scan. New tests use isolated Git/databases/processes
and explicit supervisor events, not serial-only execution (TEST-1/2/3).

Summary: no introduced documented standards violations found. This does not
certify recovery or release readiness.
