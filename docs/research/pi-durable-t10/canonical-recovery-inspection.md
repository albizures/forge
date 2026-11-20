# T10 — Canonical Rust-to-Node recovery inspection

## Status: partial; read-only host-verifier component

This increment composes the bounded recovery inspector with Forge's canonical
runtime/admission mapping and an existing exclusive kernel lock. It is **not** a
`RecoveryHost` implementation, descendant cleanup proof, lease consumer handshake,
daemon recovery controller or authorized writable reopen. T10 remains pending;
T11 and production/automatic recovery must not advance.

## Implemented

`crates/server/src/durable_authorization/recovery_inspection.rs::inspect_exclusive`:

1. Requires explicitly selected absolute Node/inspector paths and a valid Launch.
   Request serialization is capped at 16 KiB before spawning anything.
2. Authorizes Workspace scope and checks the exact persisted original Launch and
   accepted admission. Pending/unknown/rejected or different admissions cannot
   manufacture an inspection binding. It never discovers components through PATH,
   profile sources, credentials or current default selections.
3. Derives the protected runtime directory from the retained store ID and owning
   Workspace. No caller-selected storage path is accepted and no path is created.
4. Acquires the existing protected `owner.lock` nonblockingly before inspection.
   A live supervisor/worker, competing inspector, missing/unsafe evidence or path
   replacement rejects without adopting/signalling a process or repairing storage.
5. Invokes the actual read-only Node entrypoint with a cleared environment, fixed
   argv and stdin, no stderr forwarding, a 30-second observation deadline and a
   32-KiB response cap. Its directly spawned inspector cleanup is itself bounded;
   this cannot establish Assignment/descendant cleanup or universal process absence.
6. Strictly decodes version, exact Launch/admission, pending status and positive
   JavaScript-safe root ID. Unknown/missing fields and finished states reject.
7. Revalidates persisted admission/scope/mapping after the external await and
   verifies filesystem/lock identity. The returned non-serializable `Inspection`
   retains its exclusive guard through subsequent dependent operations.

`Inspection::verify` checks filesystem ownership evidence only. The result remains
a runtime observation, not authority: current Assignment/claim/freshness/retained-
work policy must still be validated in the eventual authorization transaction.
Historical read-only inspection intentionally remains possible after revocation.
No lease, audit, hold, attempt, Outcome, capacity resource or worktree is mutated.
A future controller must retain failure/effect diagnostics; errors here never clear
existing blockers or prove an absent external effect.

The internal wire request/response/status types and representation validation live
in `crates/rpc/src/durable_execution/recovery_inspection.rs`; server owns adaptation
and external wiring, not lifecycle policy. The existing effect inspector and new
recovery inspector share `inspection_process.rs` for bounded, environment-cleared
subprocess I/O. Effect observation/retention behavior and diagnostics are preserved.

## Compatibility and remaining composition

No dependency, schema/DDL, public CLI/operator RPC, RunnerRequest or Management
Protocol change. The additive typed protocol-1 subprocess contract matches the
preceding Node entrypoint. No compatibility fallback or retained Launch rewrite.
Original matched components are still required; no source-profile/model auth is
needed for these reads. This is not installed component attestation.

The inspection guard alone cannot become `authorization::Verification`. The
concrete verifier must prove predecessor descendants safe, persist protected
claim-bound evidence, apply external freshness/retained-work rules, and arrange a
verified new waiting owner without an ownership gap. Existing supervisors cannot
simply start while this inspection guard holds their lock; safe handoff/reinspection
must be explicitly composed, not implemented as drop-lock-and-assume-safety.
Lease commit/delivery, consumer handshake, daemon reconciliation, writable resume,
repeat recovery and successor demonstrations remain outstanding.

## Verification

Commands actually run from the repository root:

```sh
cargo test -p forge-server durable_authorization
cargo test -p forge-runner-host --test authorization_session recovery_inspection
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Five new Rust tests pass:

- Strict response/version/all-Launch-field/admission matching, missing/extra fields,
  wrong primitive types, nonpending states and unsafe/zero root rejection.
- Actual Forge store/canonical runtime + existing supervised-owner lock + actual
  pinned Node inspector: queued/placed repeated inspections preserve original input,
  root, attempt, resource/history/capacity state and runtime bytes; live/competing
  owners reject; the returned guard continues to exclude another inspector/host.
- Foreign Workspace, Launch, admission, relative/missing components and wrong
  inspector shape reject. Committed uncertain tool intent and completed input
  remain blocked, with unchanged Forge/runtime evidence and retained capacity.
- A trusted fault inspector changes Forge admission during the external operation
  before returning a structurally valid old reply. Post-await revalidation rejects
  it and releases only its read-only inspection guard.
- Replaced protected lock evidence invalidates the returned guard and leaves Forge
  capacity retained.

The Node seeder is a committed-storage fixture with no Harness/model/tool execution.
The directly spawned supervisor's never-granted worker is explicitly stopped by
its test controller before successful exclusive inspection. That teardown is **not
product retirement, descendant proof or a successful recovery**. Existing actual
model-process crash and external-mutation tests also pass unchanged. Runtime-byte
assertions exclude empty WAL/SHM coordination artifacts as in predecessor evidence.

Final repository gates pass: **740 Rust harness tests plus seven supervised
scenarios**, one pre-existing opt-in probe ignored. Runner typecheck and **70 Node
tests** pass. Formatting, Clippy and diff checks pass. No new packaged, real-provider
or physical power-loss verification ran. No historical transient failure is claimed
fixed; no live Assignment, operator configuration or worktree was changed.

Initial compilation exposed that server does not directly depend on serde. Wire
types were placed in the existing serde-enabled RPC boundary, with no dependency
addition. Two initial Clippy findings (documentation backticks and a test literal's
separators) were corrected before all final gates passed.

## Implementation review

T10 asks to “Wire daemon reconciliation and runner reopen through authorization,
exclusive ownership, lease issuance and interrupted-effect gates”: **partial**.
The canonical read-only inspection/lock component is implemented, but concrete
predecessor safety verification, claim-bound proof, waiting-owner handoff, recovery
consumer/daemon composition and writable resume are not. “Safe restart resumes the
same attempt/conversation with no duplicate admission” still lacks a successful
recovery demo. Continuation/independent Review successor evidence, restart matrices,
repeat recovery and glossary/ADR completion remain outstanding. No scope creep or
incorrect authority claim found.

Summary: T10 remains incomplete; authorized writable recovery is the worst gap.

## Coding-standards review

Reviewed the new RPC contract, server adapter/shared subprocess transport, changed
effect adapter, storage fixtures/tests and surrounding mapping/ownership/recovery
contracts against CODING_STANDARDS.md. Wire representation is in RPC, external
adaptation in server, and domain/persistence safety policy remains in app/store.
No external calls enter transactions. Identity/scope are checked before and after
external observation; guards remain held and serializable observations cannot
reconstruct execution authority. Subprocess arguments/environment/output are
explicit, bounded and redacted. Isolated tests cover denial, replacement, binding
change during external observation, retained capacity and byte preservation. No
secrets, ambient discovery, worktree repair or safety-policy waiver is introduced.

Summary: no introduced coding-standards violations found. Self-review only, not
independent T18 or a claim of T10/release completion.
