# T21 — Purge runtimes that went through real retirement or crash reconciliation

## Delivered outcome

Explicit runtime deletion succeeds for an eligible runtime directory produced by the real retirement and crash-reconciliation paths. All existing refusals for unrecognized, redirected or unsafe content are kept.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC16. Correction Task for T18 finding **B2** ([review](../../docs/research/pi-durable-t18/README.md#b2--blocker-explicit-runtime-deletion-fails-for-every-really-retired-or-crash-reconciled-runtime)). The responsible deliverable is T14.

`crates/runner-host/src/purge.rs` accepts only the nine names in `REMOVAL_ORDER`. Ordinary lifecycle code writes files outside that list and never removes them:

- `retirement-intent.json` (`retirement.rs`, `stop_owned`);
- `recovery-crash-<seq>.json` (`crash.rs`);
- `recovery-quiescence-{intent,receipt}.json` (`quiescence.rs`);
- `recovery-proof-*.json` (`recovery_proof.rs`);
- `owner.<pid>.pending` (`supervisor.rs`).

As a result, deletion fails with `runtime-purge-storage-unsafe` for every Assignment retired through the real path.

Recognize exactly the artifacts that runner-host itself writes. Use exact names, or strictly validated patterns with bounded numeric or identifier parts. Any other name, symlink, directory, hard link or foreign owner must still be refused. Keep the removal order: data first, then evidence, and the ownership lock last. Keep retries safe.

Also fix:

- **Audit:** the purge journal records `AlreadyAbsent` as `purged`. Record it as distinguishable from a real deletion.
- **Wire coverage:** add the `assignmentPurgeRuntime` RPC / `assignment-purge-runtime` CLI contract and end-to-end test missing from T14 (T18 M6, purge part).

Do not relax the eligibility rules: closed, reconciled, no uncertainty or hold, and exclusive ownership.

## Acceptance checks

- After a real `retire_agent_assignment` and supervised reconciliation, purge removes the mapped runtime directory and leaves Forge audit and the worktree unchanged.
- After the crash-reconciliation path, purge succeeds on the files it left.
- Unknown names, malformed pattern matches, symlinks, hard links and wrong-owner files are still refused before anything is removed.
- The journal distinguishes a real removal from an already-absent directory.
- The CLI/RPC purge contract is tested end to end, including scope denial, ineligibility, exact retry and conflicting retry.

## Verification and evidence

Add composed real-process tests that do not use SQL-inserted eligibility:

- real retire → reconcile → purge;
- crash → reconcile → retire → purge.

Add host unit tests for each pattern and for near-miss names, and a CLI → daemon → host purge test.

Run all four repository gates, runner typecheck and the Node tests. Record the results in `docs/research/pi-durable-t21/README.md`. Update `crates/store/README.md` if the journal representation changes, and advance the schema revision for any breaking storage change (CONTRACT-1).

## Dependency

Depends on T17. Code Stack predecessor: T17.
