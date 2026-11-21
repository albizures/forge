# T21 — Purge runtimes after real retirement and crash reconciliation

Corrective Task for T18 **B2** (and the purge part of **M6**). Controlled deliverable. Production
durable dispatch and automatic recovery remain disabled.

## Problem (T18 B2)

`runner_host::purge` accepted only nine exact names. Real lifecycle code writes other files
into the runtime directory and never removes them, so explicit purge failed closed with
`runtime-purge-storage-unsafe` for every Assignment retired through the real path. The only
composed purge test used SQL-inserted eligibility, so these files never appeared.

## Delivered

- **Host recognition (`crates/runner-host/src/purge.rs`).** Each entry is classified before
  anything is removed. Accepted names are exactly those runner-host and the pinned runner write:
  - exact names: `runtime.sqlite{,-wal,-shm,-journal}`, `check-runtime.json`, `owner.json`,
    `owner.lock`, `watchdog.json`, `retirement-intent.json`, `retirement-receipt.json`,
    `recovery-quiescence-intent.json` and `recovery-quiescence-receipt.json`;
  - bounded patterns:
    - `recovery-crash-<u32 ≥ 1>.json` (`crash.rs`);
    - `recovery-proof-<64 lowercase hex>.json` (`recovery_proof.rs`);
    - `owner.<u32 ≥ 1>.pending` (`supervisor.rs`, left only by an interrupted write);
    - `watchdog-<u32 ≥ 1>-<u64>-<term|kill>-<sent|unverified>.json` (`watchdog.rs`).

  Numeric parts must be canonical decimals: no sign, no leading zero, in range. Any other
  name, and any symlink, directory, hard link (`nlink != 1`) or foreign-owned entry, refuses
  before anything is removed.

  Removal follows a total order: data, configuration, diagnostics, recovery evidence, fencing
  markers, the retirement receipt, owner evidence, and `owner.lock` last. Within a phase,
  files go in name order. Retry semantics are unchanged.
- **Journal (`durable_runtime_purge_removals`, schema revision 24).** A completed purge
  records whether this operation `removed` the directory or found it `already-absent`.
  `Finish::Purged { removal }` and `Record::removal` carry that result through app, store,
  RPC and CLI. Revision 24 adds a separate table so that revision 23 journal rows upgrade
  byte-exact:
  - triggers allow an insert only for a `purged` row and reject UPDATE and DELETE;
  - a purge completed before revision 24 reads `removal: null`, rendered as "not recorded",
    never as a fabricated deletion.

  The explicit offline upgrade from 23 adds the empty table. `SCHEMA_REVISION` is now 24, so
  ordinary open of a revision 23 store fails with `fresh-store-required` (CONTRACT-1).
- **Server** passes the host's `Purged::{Removed, AlreadyAbsent}` through. **CLI** human output
  (`assignment-purge-runtime` and `assignment-status`) states which one happened. RPC re-exports
  `RuntimePurgeRemoval` and `RuntimePurgeState`.
- **Eligibility is unchanged:** the Assignment must be closed, reconciled or rejected, have no
  unretired resource and no Dispatch Hold, and have exclusive ownership. Real retirement leaves
  a Dispatch Hold, so purge stays refused (`runtime-purge-uncertainty-retained`) until the
  operator runs `forge dispatcher resume <task-id>`. This is now documented.
- **Docs:**
  - `docs/assignment-recovery.md`: recognized artifacts, removal order and the removal outcome.
  - `docs/durable-execution.md`: the hold step and the removal outcome.
  - `crates/store/README.md`: the revision 24 table and upgrade semantics.

## Evidence

- **Host unit tests (4 new, 9 total in `purge.rs`):**
  - every pattern is classified at its rank;
  - 40 near-miss names are rejected: leading zeros, sign, overflow, zero sequence or PID,
    upper-case or short/long hex, wrong or extra segments, unknown signal or result, `.tmp`
    suffixes, path-like and empty names;
  - a directory holding every real lifecycle artifact is removed in rank order with the lock
    last;
  - a near-miss name, a symlinked lifecycle name, a hard-linked proof, a directory named like
    a diagnostic, or a hard-linked pending file each refuse with the directory listing unchanged.
- **Store tests:**
  - `Removed` is recorded and replayed;
  - `AlreadyAbsent` is recorded and survives a reopen;
  - a later different finish does not rewrite the result;
  - a removal row cannot be inserted before completion and cannot be updated or deleted;
  - a revision 23-style completion reads `removal: None`;
  - the upgrade test `purge_removal_expansion_is_empty_and_stamp_failure_preserves_schema_twenty_three`
    covers 23 → 24 with stamp-failure rollback, byte-exact preserved tables and an empty new table;
  - all existing upgrade fixtures now target 24.
- **Composed real-process tests (`runner-host/tests/authorization_session/purge_runtime.rs`),
  with no SQL-inserted eligibility:**
  - `real_retirement_and_reconciliation_leave_a_purgeable_runtime`: real supervisor, worker and
    registered controller → `Application::retire_agent_assignment` → reconciled retirement with
    `runner-retirement-verified`. Purge is refused while the retirement hold stands; the
    operator clears it with `resume_dispatch`. The directory holds `retirement-intent.json`,
    `retirement-receipt.json`, `owner.json` and `owner.lock`. Purge returns `Purged` with
    `Removed`; an exact retry replays and a different operation conflicts. Forge audit tables
    and worktree bytes are unchanged.
  - `crash_reconciliation_then_retirement_leaves_a_purgeable_runtime`: real worker kill →
    original subreaper crash proof → daemon `reconcile_agent_dispatch` recovery, including a
    second proven crash → retirement → purge. That path leaves:
    - `recovery-crash-1.json` and `recovery-crash-2.json`;
    - two `recovery-proof-<hex>.json` files;
    - `retirement-intent.json` and `retirement-receipt.json`;
    - `runtime.sqlite{,-wal,-shm}`;
    - the owner files.

    All of them are deleted with the same assertions as the first test.
  - **Regression check:** with the previous `purge.rs` restored, both new tests fail with
    exactly `runtime-purge-storage-unsafe`. With the correction, both pass.
  - Synthetic test workers also write `controlled-*` fixture files, which production never
    writes. The tests remove only those files before purging.
- **CLI → RPC → daemon → host (`cli/tests/support/supervised_purge.rs`, run in the `retired`
  supervised scenario after the actual `assignment-retire`):**
  - foreign Workspace → `durable-scope-denied`, with no journal;
  - retirement hold → `runtime-purge-uncertainty-retained`, with no journal;
  - CLI `resume` clears the hold;
  - fixture content → `runtime-purge-storage-unsafe`, with nothing removed and status showing
    `retained` with the diagnostic;
  - a conflicting operation ID → `runtime-purge-conflict`;
  - after the fixture files are removed, the exact retry → `purged`/`removed` and the
    directory is gone;
  - a second exact retry returns the identical record without recreating storage, and the
    conflict is refused again;
  - Assignment info, Checks and worktree bytes are unchanged;
  - human output states the removal, and status shows `purged`/`removed`.
- **CLI render unit test:** removed, already-absent, unrecorded and pending render distinctly.

## Gates

Run from the repository root with real Node 24.18.0 first on `PATH`, because the Volta shim
breaks the tests that resolve `node` from `PATH`. An initial supervised-tracer run under the
shim failed at that probe; the gates below use the real binary.

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `git diff --check` are clean.
- `cargo test --workspace --no-fail-fast`: 890 passed, 0 failed, 1 pre-existing opt-in probe
  ignored, including all 10 supervised tracer scenarios.
- Runner `npm run check` (tsc) is clean. `npm test`: 90 passed, 0 failed. Runner sources are
  unchanged.

## Not claimed

- A **wrong-owner** file cannot be created without root in these tests. The `uid` check is
  unchanged and covered by code review only, as in T14.
- On this crash path, quiescence files are not produced. `recovery-quiescence-*` and
  `watchdog-*` diagnostics are covered by the host unit tests, not by a composed run.
- `already-absent` is exercised in store and render tests and in the host `purge(None)` unit
  test, not through a composed retry where an earlier attempt removed the directory but crashed
  before `finish_purge`. That case records `already-absent` by design, because this operation
  removed nothing.
- `assignment-transcript` after purge is covered by the existing host composition. The test
  daemon has no transcript reader components.
- B1 (no production durable launch path), M1–M5 and the follow-up part of M6 are outside this
  Task (T22–T24, then the T18 re-review).
