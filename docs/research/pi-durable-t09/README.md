# T09 — Retirement finalization increment

## Status: partial; T09 remains pending

The first pending Task is T09. This increment implements the conservative
application/store finalization boundary, **not supervised durable retirement**.
T10 must not start. Production durable dispatch and automatic recovery remain
disabled. No live Forge records, profiles, provider resources or worktrees were
changed. Existing local planning changes were preserved.

## Implemented

- `Application::retire_agent_assignment` retains its existing hold → persistent
  retirement marker → closure/revocation → original-profile adapter ordering.
  After adapter success it now calls a specialized finalization operation instead
  of unconditionally retiring the marker.
- Application policy requires revoked authority and no unresolved resource other
  than the exact `retirement-pending/operator` marker. The store checks these
  facts and retires that marker in one immediate transaction, after external I/O.
- Unknown launches, durable runtimes, interrupted effects, authorization/ownership
  uncertainty, surviving provider resources and differently identified retirement
  markers block finalization. A successful cancellation/abort is not absence.
- Generic provider resource retirement cannot finalize retirement markers.
  Repeated successful finalization preserves the original timestamp, including
  after store reopen. No resource history, unknown-resolution audit or hold is
  deleted. Storage failure retains revocation, the pending marker and capacity.
- The existing CLI/RPC now returns `assignment-retirement-unreconciled` rather
  than successful retirement output when cleanup leaves retained uncertainty.
  After explicit unknown-resource confirmation, the operator must retry retirement
  and separately review/resume the hold. Known Herdr cleanup and Grilling routing
  remain on their original adapters; historical cleanup does not rerun preflight.

No schema, runtime-schema, dependency, serialized-field or RPC-method change.
This intentionally tightens an existing command's success/error semantics and the
internal repository contract. CLI/RPC integration tests and the
[recovery runbook](../../assignment-recovery.md#revoke-and-clean-up-known-resources)
were updated together. No host process-death proof is inferred from these facts.

## Verification

From the repository root, on the final code snapshot:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd runner && npm run check && npm test)
git diff --check
```

All passed: **687 Rust harness tests, 6 custom-harness supervised tracer scenarios,
52 Node tests; 1 pre-existing opt-in Rust probe ignored**. Runner typecheck passed.
The five added tests cover live-authority denial, generic-finalization denial,
exact retry timestamps, all retained uncertainty classes across reopen, unknown
confirmation/hold preservation, transactional failure and an actual application →
SQLite path with an adapter returning success without verifying cleanup. Existing
CLI/RPC tests cover failure output, revocation, retained capacity, confirmation,
retirement retry, explicit resume and original-backend routing. Protocol adapters
in the new tests are fakes, not real-process retirement evidence.

Earlier runs exposed expectations for the old success-with-unresolved-resources
behavior; those tests/fixtures now use atomic finalization or assert the retained
error and explicit reconciliation sequence. An earlier workspace run also timed
out in `interrupted_external_mutation_retains_forge_blocker_across_daemon_restart`
at `crates/cli/tests/durable_check_bootstrap.rs:746`. Subsequent full parallel runs
passed; the timeout was not diagnosed or fixed and is not claimed resolved.
No packaged-component, clean-install or real-provider release verification ran.

## Reviews on this increment

### Implementation review

Against T09: **partial**. “Stop admission, cancel runtime execution, terminate the
supervised process group and verify relevant cleanup” is not implemented by this
increment. “Run real-process tests for long-running children, delayed shutdown,
surviving/escaped child evidence and process-identity mismatch” remains unfulfilled
for retirement, as does protocol/storage fault injection across those host steps.
The finalization transaction and adapter-success regression implement one required
safety gate, not a complete replacement for those mechanisms. No out-of-scope
recovery, replay, resource deletion or rollout was added.

Summary: T09 cannot be completed; the worst gap is the missing supervised
cancellation/verified cleanup path.

### Coding-standards review

Reviewed the changed application/store modules, tests and operational contract
against `CODING_STANDARDS.md`. Policy remains in app; the store owns transactional
validation and writes. External calls stay outside the transaction. Failures retain
uncertainty; retries preserve timestamps; the marker cannot be generically retired.
No new dependencies, secrets, shell operations or storage compatibility changes.

Summary: no introduced coding-standards violations found in this increment.
These are local self-reviews, not T18's later independent review.

## Remaining T09 work (before advancing)

1. Integrate the existing operator retirement path with identity-bound supervisor
   admission fencing and runtime cancellation, only after durable revocation.
2. Terminate only verified owned runner/process-group execution; verify relevant
   cleanup rather than interpreting abort/kill success, runner exit, EOF or lease
   expiry as proof. Retain escaped/surviving-child uncertainty and capacity.
3. Persist/reconcile the original retirement operation across cancellation,
   termination and final result failures/restarts, without adopting lookalikes.
4. Add the Task's real-process and fault matrix, including canonical runtime and
   tracked/staged/untracked Git preservation. Existing generic resource retirement
   is still a trusted provider port, not a new verified durable-cleanup receipt.
5. Complete/review the retirement portions of ADR-0013 through the domain-modeling
   workflow, rerun affected gates, then decide T09 completion. No ADR completion
   or permission to release a durable runtime is claimed here.
