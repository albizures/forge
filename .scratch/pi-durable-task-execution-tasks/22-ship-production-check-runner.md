# T22 — Ship a production Check runner entry and a real recovery entry

## Delivered outcome

The packaged runner entry is a real Check runner. It does what the test workers do today:

- composes protected capability delivery, bootstrap, the explicit durable profile, model authentication and the approved real tools;
- admits work only under the existing versioned runner contract and authorization lease;
- resumes through a recovery entry whose input contract is actually written and tested.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC3, AC4, AC5, AC7, AC8, AC17, AC18. This is a correction Task for T18 finding **B1**, in the runner part ([review](../../docs/research/pi-durable-t18/README.md#b1--blocker-no-production-durable-launch-or-execution-path-exists)), together with the protocol part of **M5**. Responsible deliverables are T04, T07 and T10.

Today:

- `crates/forgeup/src/runner.rs` packages `ENTRY_RELATIVE = "runner/src/main.ts"`. That file is the synthetic T04 runner: a faux provider, no bootstrap, no profile.
- The bootstrap-gated, real-tool compositions exist only as `runner/test/supervised-check-worker.mjs`, `recovered-supervised-check-worker.mjs` and `check-bootstrap-tracer.mjs`.
- `runner/src/check-recovery.ts` is never executed, and nothing writes its `check-runtime.json` input.

Move these compositions into shipped source under `runner/src/`. Keep their existing safety behaviour:

- bootstrap happens before any model or tool is built;
- the provider and every tool are lease-guarded, with retries and compaction off;
- interrupted-effect inspection happens before resume;
- the read-only transcript reader is used;
- there is no ambient Pi discovery.

Then:

- Point `ENTRY_RELATIVE` at the production entry. Keep the synthetic runner only as a test fixture.
- Define who writes the recovery input and how it is validated. Make `check-recovery.ts` the tested recovery entry, or remove it in favour of one entry that handles both admission and recovery.
- Use one versioned runner contract. Remove the wire-v1 variants the runner rejects (`Transcript`, `Recover`, `FollowUp`, `Retire` → `runner-operation-unavailable`), or make them the real path. Parse runner events into typed values at the Rust boundary (RUST-1).

The test workers should become thin wrappers that only swap in the faux provider. They must not be a second implementation.

Production dispatch stays refused in this Task; T23 composes the server side. Do not change lease timing, effect policy or authentication scope.

## Acceptance checks

- The packaged entry refuses execution without successful bootstrap, a valid profile, model authentication and a lease.
- Tests use it with only the provider substituted.
- The recovery input is written by named production code, validated strictly, and covered by a real-process recovery test that runs the shipped recovery entry.
- One versioned contract covers admission, observation, recovery, follow-up and retirement, with no defined-but-rejected operations.
- Existing AC3/AC8/AC17/AC18 runner tests pass against the production entry, including the secret-canary scans.
- Synthetic-only code is no longer reachable as the packaged entry.

## Verification and evidence

Re-point the existing Node tests, the `durable_check_bootstrap` suite and the supervised tracer workers at the shipped modules. Add a recovery test that runs the shipped recovery entry from its real input file.

Run all four repository gates, runner typecheck and the Node tests. Record the results in `docs/research/pi-durable-t22/README.md`, and correct the T10 evidence statement about `check-recovery.ts`. Assess wire compatibility explicitly (CONTRACT-2).

## Dependency

Depends on T21. Code Stack predecessor: T21.
