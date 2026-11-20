# T10 — Bounded committed recovery inspection entrypoint

## Status: partial; no recovery permission

This increment exposes the existing read-only runtime candidate check to a future
trusted host verifier. It does not implement `RecoveryHost`, predecessor cleanup,
lease consumption, daemon reconciliation or writable reopen. T10 stays pending;
T11 and production/automatic recovery remain blocked.

## Internal contract

`node runner/src/inspect-recovery.ts RUNTIME_DIRECTORY` consumes one JSON document
on stdin, at most **16,384 bytes**, with exactly these fields:

```json
{"protocol":1,"launch":"<complete original Launch object>","admission_id":"<original accepted input ID>"}
```

The `launch` placeholder above must be replaced with the full object, not a string.
Unknown fields, extra command arguments, incompatible protocol/schema/component,
foreign Launch/admission, malformed input and unavailable storage fail closed.
The explicitly selected Node/component paths and cleared environment belong to
the future trusted caller; this is not an operator RPC or an Assignment action.

Success emits one JSON line containing exactly `protocol`, `launch` and
`candidate`; candidate contains `root`, `admission_id` and `status` (`queued` or
`placed`). It contains no input, Task body, command arguments, tool output or
credentials. Failure exits 1, emits no stdout and only the fixed stderr diagnostic
`runner-recovery-inspection-failed`. No exception or caller content is disclosed.

The existing helper validates protected storage, the complete journal Launch,
committed unowned root, whole-store interrupted effects and the exact original
pending input. The entrypoint imports no Harness and cannot submit input, start
scheduling, repair tasks or initialize missing storage. Finished/failed inputs
remain intervention states, not permission to manufacture another submission.

The caller must bound subprocess duration/output, authorize scope and canonical
mapping, prove predecessor descendants safe, and continuously hold exclusive
ownership across inspection and any dependent authorized operation. This command
does **not** acquire that lock or prove process absence. A successful standalone
snapshot is not stable execution permission. Any eventual host verifier must also
persist protected claim-bound evidence and apply current Forge/freshness policy.

## Compatibility

No dependency, DDL/schema, public CLI/RPC or existing RunnerRequest contract change.
This is an additive internal protocol-1 read-only command. The component digest
now includes both `src/recovery.ts` and `src/inspect-recovery.ts`, so edits to either
change compatibility identity. Old retained Launch hashes are not rewritten or
silently accepted by this component; matching historical inspection components
are still required. This is local compatibility checking, not T15 installed
component attestation.

## Verification

Commands actually run from the repository root:

```sh
(cd runner && npm run check && node --test --test-timeout=20000 test/recovery.test.mjs)
(cd runner && npm run check && npm test)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Final full runner run: typecheck and **70 Node tests pass**. Six new wire tests
cover queued/placed exact retries, malformed/oversized requests, unknown fields,
protocol/component/binding/admission rejection, missing/corrupt stores, finished
inputs, child-conversation effect intent, fixed redacted failures and unchanged
runtime bytes. The child effect is a committed-storage fixture, not another real
external mutation test.

The existing real model-process SIGKILL test now also invokes the actual inspector
subprocess three times after death. It finds the same original placed input and
root without a second model entry, writable reopen, submission or runtime-byte
change. This is real model-process death/read-only observation, **not** supervised
recovery, descendant safety proof or physical power-loss evidence. SQLite's empty
WAL/SHM reader coordination files remain excluded as in the predecessor tests.

Rust formatting, Clippy and the full default-parallel workspace suite pass:
**735 Rust harness tests plus seven supervised scenarios**, one pre-existing
opt-in probe ignored. `git diff --check` passes. No packaged or real-provider test
ran. Historical transient failures are not claimed fixed. No live Assignment,
profile, capability, configuration or worktree was changed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
The new subprocess exposes only committed runtime preconditions. Concrete host
verification, a recovery consumer/daemon controller and authorized writable open
remain missing. “Safe restart resumes the same attempt/conversation with no
duplicate admission” still lacks a successful recovery demonstration. Continuation/
Review successor demonstrations, repeated recovery, restart matrices and domain/
ADR completion remain outstanding. No scope creep or incorrect authority claim
was found.

Summary: T10 remains incomplete; the worst gap is authorized writable recovery.

## Coding-standards review

Reviewed the new entrypoint, surrounding recovery/storage/effect contracts,
component-hash change, crash fixture and wire tests against CODING_STANDARDS.md.
External inspection stays separate from policy/transactions; no safety gate is
weakened. Tests isolate runtime directories/processes, use event-based crash
synchronization, check redaction and preserve byte evidence. No source-profile,
credential, mutable wire/schema or worktree-repair path is added.

Summary: no introduced coding-standards violations found. This is a self-review,
not independent T18 or T10/release completion.
