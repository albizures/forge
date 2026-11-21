# T14 — Retained runtime history and explicit purge

Controlled deliverable. Production durable dispatch and automatic recovery remain disabled.

## Delivered
- Retirement is unchanged: it never deletes runtime history, which stays inspectable (owner-only directory/files, checked by the host on every open).
- App `durable_execution::purge` (request, record, `Repository`); store `durable_runtime_purges` (in unreleased schema 23; empty on upgrade; immutable identity, final once `purged`, undeletable) and an atomic eligibility check plus idempotent journal; host `runner_host::purge` (exclusive owner lock, recognized-file allowlist, no-follow/ownership/link checks, data → `owner.lock` → directory order, resumable) with `storage::existing_runtime_directory`; server `durable_authorization::purge`; RPC `assignmentPurgeRuntime`; CLI `forge dispatcher assignment-purge-runtime`; status shows the record; `transcript::page` reports `runtime-history-purged` without a reader.
- Operator documentation in `docs/assignment-recovery.md`.

## Evidence
- Host unit tests (5): removal of only recognized files and directory; unrecognized and symlinked entries delete nothing; live lock owner blocks and deletion succeeds after release; redirected directory; interrupted-remainder resume, with the refusal of a lockless directory that still has content.
- Store tests (3): open/unreconciled/foreign rejected with no journal; unknown resource and Dispatch Hold block even after reconciliation; journal idempotence, conflict, retained-then-retry, final purge, trigger protection and identical Forge audit tables before/after.
- Composed `runner-host/tests/authorization_session/purge_runtime.rs`: real supervisor and worker owning a real runtime directory and a real store. Open Assignment rejected; live owner refused and journaled `retained`; a killed supervisor whose worker still holds the lock is still refused; unrecognized fixture content refuses untouched; after removal, exact retry purges once and replays; the neighbouring runtime and worktree bytes and all Forge audit tables are unchanged; transcript after purge is `unavailable`/`runtime-history-purged` with reader paths that do not exist.
- Upgrade fixtures include the new empty table.

## Gates
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `git diff --check` are clean. `cargo test --workspace --no-fail-fast` with a real Node 24 first on `PATH`: 865 passed, 0 failed, 1 pre-existing opt-in probe ignored. An earlier full run had two failures, both fixed and rerun: a `[--json]` placeholder in the new docs rejected by the operator-guide parser, and a racy test assertion that the owner's process group still existed (the worker can exit with its supervisor). The runner TypeScript was not changed, so runner typecheck/Node tests were not rerun.

## Not claimed
- No CLI/daemon end-to-end run of `assignment-purge-runtime` (the service and store are composed in the host test; the RPC/CLI adapters are thin and only compile- and help-checked).
- No fault injection of a filesystem error mid-deletion beyond the unrecognized-file and resume cases; concurrent reader/writer races rely on the exclusive lock (readers take no lock and see `unavailable` if the files vanish).
- Same-user malicious filesystem writers are out of scope, as for the existing inspection guard.
