# T10 — Read-only original-worktree progress verification

## Status: partial component, not recovery authority

This increment adds the repository-progress side of trusted recovery verification.
It does **not** implement `RecoveryHost`, persist the protected combined proof,
transfer executable ownership, reconcile the daemon or reopen a writable Harness.
The existing positive recovery receiver tracer still uses synthetic external
verification. Production dispatch and automatic recovery remain disabled.

## Delivered boundaries

- `app::durable_execution::recovery::worktree` owns the policy and observation
  coordinator. A snapshot binds the exact active candidate to the Project root,
  fixed worktree placement, Base Commit and current Check name. The snapshot is
  not deserializable authority and its content/index hash is not accepted evidence.
- The store reads those facts and the content-bound active claim in **one SQLite
  snapshot**, reusing the original candidate gate. Source profile files and model
  authentication are not consulted. Foreign scope, revoked/stale/cancelled work,
  missing Base Commit and mismatched placement/repository fail closed.
- The server adapter checks canonical placement, the original repository, the
  deterministic Task branch and ancestry of **both** the persisted Base Commit
  and original attempt input. It fingerprints HEAD, index semantics and retained
  working/untracked bytes using the existing v2 retained-state observer.
- Implementation may have dirty/staged/untracked or committed descendant progress.
  This remains the original attempt's *observed progress*, not a checkpoint,
  salvage authorization, freshness update, Assessment or adopted output evidence.
- Other Checks, including Review and Publication, require their exact clean bound
  input. Assume-unchanged and skip-worktree index flags are conservatively dirty:
  ordinary Git status cannot certify hidden working bytes as clean review input.
- Inspection compares two external observations with current Forge reads before
  and after the second observation. Its retained result can be revalidated after
  other external I/O; changed content, bindings or authority invalidate it.
  It neither acquires an execution lock nor proves process absence. A future
  controller must hold the verified waiting-owner lock through verification,
  proof persistence, grant commit and delivery. Repeated reads alone are not
  protection against an arbitrary concurrent writer or a changed-then-restored
  observation, and this read-only object is not a terminal authorization fence.

No code commits, resets, cleans, stashes, fetches, runs setup hooks, repairs Git,
changes worktree claims or rewrites attempts/evidence to make recovery eligible.
Worktree removal/upstream safety is deliberately not used as an execution gate.

## Ambient executable regression and supported Git inputs

A new actual-Git test configured a marker-writing fsmonitor program. It **failed**
because the existing retained-state helper's `git ls-files` could execute that
program even with `--no-optional-locks`. The helper and new status reader now
explicitly disable `core.fsmonitor`; the same test passes without creating the
marker. This correction applies to the shared retained-state read path as well
as the new recovery verifier. There is no lint suppression or timeout workaround.

Configured clean/process filter drivers are rejected before status inspection,
not discovered by executing them. The v2 retained-state reader rejects unsupported
links, directories/gitlinks (including submodules), unreadable files and unknown
Git state before status can descend into nested repositories. These remain
unsupported observations, never permission to repair or silently weaken evidence.
This is trusted-local-host inspection, not hostile same-user containment. Git
availability, inherited configuration and external writers still belong to that
boundary; there is no claim of bounded Git subprocess/file-size budgets here.

There is no schema, wire, dependency, CLI or capability change. The new internal
read port is additive. Historical evidence is not rewritten or fingerprint hashes
backfilled. Disabling ambient fsmonitor may expose differences in previously
observed index flags/retained fingerprints; such mismatches block existing salvage
revalidation and require investigation, not automatic acceptance/reset.

## Verification actually run

```sh
cargo test -p forge-store --lib recovery_worktree
cargo test -p forge-server --lib recovery_worktree
cargo test -p forge-server --lib recovery_worktree_real_store
cargo test -p forge-server --lib recovery_worktree_never_invokes
cargo test -p forge-server --lib retained_state
cargo test -p forge-server --lib registered_grilling_receives_protected_capability_from_production_adapter
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

Final gates pass: **809 Rust harness tests**, seven custom-harness supervised
scenarios, one pre-existing opt-in ignored; runner typecheck and **76 Node tests**
pass. Twelve new Rust tests were added from the preceding 797-test snapshot:

1–2. Actual store snapshots across reopen, exact candidate binding, scope/conflict
and revocation denial, missing/malformed Base Commit, evidence/placement drift,
and full logical database snapshots proving read-only success/failure.

3. Actual SQLite + actual Git composition with synthetic original admission/lease
history: original attempt/evidence/work state, retained capacity, index and working
bytes remain unchanged by observation; revoked authority rejects revalidation.
No real process-safety or runtime-reopen proof is claimed by this fixture.

4–12. Actual Git Implementation progress; exact clean Review/Publication inputs;
hidden index flags; wrong/detached branch, repository, base and original-input
ancestry; revocation during the final external observation and later binding drift;
unsupported submodules; a writer changing content between observations; disabled
fsmonitor/rejected filters; symlinked placement and unsafe files. Most of these
use a deterministic fake snapshot port, not the store.

Initial compilation needed the repository's signed SQLite integer conversions.
One negative store fixture attempted to mutate an immutable Project root and was
correctly rejected by its trigger; it now injects changed attempt evidence instead.
Clippy later rejected test-local items after statements; the two distinct cases
were separated into tests without suppression.

The first full workspace run failed the existing Herdr protected-capability test
with `ExecutableFileBusy` while spawning its copied test executable. That target
and two subsequent complete default-parallel workspace runs passed without a
change to the Herdr test or its implementation. The second full pass is on the
final code snapshot (including the additional race/submodule tests). The transient
failure is retained here; its cause is not claimed diagnosed or fixed.

No writable-recovery, real-provider, packaging or power-loss verification ran.

## Implementation review

T10 asks that “Current worktree/profile observations must follow existing
freshness/retained-work rules without treating expected implementation progress as
automatically adopted completion evidence.” This increment supplies the read-only
repository observation/policy component and preserves that distinction.

“Wire daemon reconciliation and runner reopen through authorization, exclusive
ownership, lease issuance and interrupted-effect gates” remains partial: the
worktree verifier is not composed with the waiting-owner inspector and protected
combined proof into a real `RecoveryHost`. “Safe restart resumes the same
attempt/conversation” is still unproved. Full restart-order recovery matrices,
Continuation/independent Review successor evidence and glossary/ADR completion
remain outstanding. No unrelated feature or recovery-completion claim was added.

Summary: T10 remains pending; trusted proof/handoff orchestration and executable
writable recovery are the worst gaps. Self-review, not independent T18.

## Coding-standards review

Reviewed the new app/store/server modules, changed shared retained-state helper and
new tests against `CODING_STANDARDS.md`. Policy stays in app, transactional reads
in store and Git/process operations behind the server inspector (ARCH-1/2).
External I/O stays outside transactions, followed by live snapshot revalidation
(TX-4). Unknown observations retain original state/capacity; no cleanup/adoption
side effects exist (SAFE-2/3/4). The shared fsmonitor correction has a demonstrated
pre-fix failing behavioral regression; tests isolate Git directories/databases and
inject timing through deterministic ports (TEST-1/2/3). No introduced documented
standards violations found. These checks do not certify release readiness.
