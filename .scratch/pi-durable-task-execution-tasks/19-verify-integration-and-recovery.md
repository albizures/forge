# T19 — Verify packaged integration, recovery and a real-provider Check cycle

## Delivered outcome

A retained verification report establishes whether the reviewed, packaged backend satisfies the complete Spec, including failure paths and real authenticated evidence handoff.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1–23, particularly AC22. Build on T18; unresolved review blockers prohibit a passing result. This is the explicit post-implementation/local integration Verification Task required by the planning guide.

Run repository gates from the Forge root:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` in normal parallel mode
- `git diff --check`
- Runner type/build/test and packaging commands established by the implementation, plus shipped-skill gates if affected.

Run deterministic cross-process fault scenarios: duplicate admission, competing writer, daemon/runner restart ordering, lease expiry, stale/revoked recovery, interrupted external effect, missing Outcome, source loss, incomplete retirement, profile migration retries, deletion safety, packaging mismatch and rollback. Exercise the documented CLI/RPC operator controls and retained Herdr/Grilling behavior.

Use an isolated authorized real-provider environment for Implementation → independent Review: actual authenticated bootstrap/Outcomes, changed output commit, exact Review binding and retirement before successor. Cover review handback/Continuation and publication/unchanged rules through the applicable retained integration gates. Never target ordinary live work or embed credentials in evidence.

## Acceptance checks

- Evidence maps each AC to commands, expected/actual results and relevant retained artifacts.
- Report names full Forge Revision/build state, runner/protocol/dependency identities and reviewed snapshot.
- Unrun, failed or unavailable gates are explicit; they withhold default-rollout approval.
- No external-effect duplication, revoked execution, unsafe capacity release or inferred completion is observed.

## Verification and evidence

Deliver reproducible logs/report with redaction, environment description and remaining limitations. Local passing checks alone are not release approval. Any material implementation correction invalidates affected results and requires renewed review/verification before proceeding.

## Dependency

Depends on T18. Non-code Verification Task; ordinary dependency only.
