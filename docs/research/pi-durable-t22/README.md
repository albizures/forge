# T22 — Ship a production Check runner entry and a real recovery entry

Corrective Task for T18 **B1**, runner part, and the protocol part of **M5**
([review](../pi-durable-t18/README.md#b1--blocker-no-production-durable-launch-or-execution-path-exists)).
Controlled deliverable. Production durable dispatch stays refused: the server adapter is T23 and
packaging of `forge-runner-host` is T24. Lease timing, effect policy and authentication scope are
unchanged.

## Problem

- Forgeup packaged `runner/src/main.ts`, the synthetic T04 runner: a faux provider, no bootstrap,
  no profile.
- The real compositions (bootstrap → profile → guarded tools → Harness) existed only in the test
  workers `supervised-check-worker.mjs`, `recovered-supervised-check-worker.mjs` and
  `check-bootstrap-tracer.mjs`.
- `runner/src/check-recovery.ts` was never executed, and nothing wrote its `check-runtime.json`.
- Wire v1 defined `Transcript`, `Recover`, `FollowUp` and `Retire` request operations that the
  runner always rejected (`runner-operation-unavailable`). Runner events were matched as untyped
  `serde_json::Value` strings in the daemon recovery driver (RUST-1).

## Delivered

### One shipped entry for admission and recovery

- **`runner/src/check-runner.ts`** (new) is the production Check runner composition.
  `runCheckRunner(directory, worktree, substitute?)` reads the protected input, answers
  `{"host_ready":true}` with `owned`, then handles, on one stdin:
  - first admission: `host_control` grant, then `RunnerRequest {admit}` → `admitCheck`;
  - observation: `RunnerRequest {observe}` (read-only, also without a lease);
  - same-attempt recovery: `host_prepare_recovery`, `host_recovery`, renewals → the shared
    `RecoveryConsumer`, whose `prepare` callback re-runs `composeCheckRuntime` from the same input
    after checking that the candidate's launch and attempt equal the original binding;
  - recorded follow-ups (`host_followup`) in either mode.

  Recovery and first admission are mutually exclusive in one incarnation. Any failed admission
  fences the incarnation, so a retry cannot re-run bootstrap.
- **Safety order is the shipped one.** `composeCheckRuntime` calls the existing
  `createCheckRuntime`:
  1. interrupted-effect inspection;
  2. bootstrap;
  3. explicit profile and model-key loading;
  4. guarded provider and the four guarded, replay-unsafe coding tools, with retries and
     compaction off and no ambient Pi discovery.

  Only then are the runtime intent (`persistIntent`) and the writable store created. Recovery
  still uses `resume-runtime.ts` (effect and candidate inspection before and after `prepare`, a
  read-only reader for the stored agent cwd).
- **Admission results are typed.**
  - A failure before intent is `admission: {state: "rejected", code}`, for example
    `assignment-runtime-failed` for a failed bootstrap, model authentication or profile.
  - A failure after intent is `{state: "unknown"}`, never rejected.
  - An uncorrelatable request gets `{"error":{"code"}}`.
- **`runner/src/check-entry.ts`** (new) is the packaged bin: `runCheckRunner(directory, worktree)`
  with no substitution. `crates/forgeup/src/runner.rs` sets
  `ENTRY_RELATIVE = "runner/src/check-entry.ts"`.
- **The synthetic runner is a test fixture only.** It moved to `runner/test/synthetic-runner.ts`.
  `package-bundle.mjs` copies only `runner/src/`, so it cannot be packaged. It remains the receiver
  fixture for the host-side recovery handoff tests (`recovery_receiver.rs`, `recovery_handoff.rs`),
  which need a receiver that does not resume.
- **`runner/src/check-recovery.ts` is removed** in favour of the single entry.
- **`recovery-consumer.ts`** is refactored into `RecoveryConsumer` (same checks and events). The
  old `serveRecovery` loop is a thin wrapper over it, for the existing low-level recovery
  fixtures.
- **`componentHash()` now attests every `runner/src/*.ts`** (sorted), plus the lock and
  `prepare-reader.mjs`, so a new module cannot escape attestation.

### Recovery input: who writes it and how it is validated

- **Shape.** `check-runtime.json` is the typed, versioned `forge_rpc::durable_execution::CheckRuntimeInput`
  (`deny_unknown_fields`): `version: 1`, `launch`, `generation`, `bound` (`BoundIdentity`),
  `configuration` (`ExecutionConfiguration`), `credential_file` (a path) and `capability`
  (metadata and the secret-file path). It never contains key bytes, capability bytes, behavior or
  a lease. `Debug` omits the paths.
- **Writer.** `forge_runner_host::check_runtime::write` (new) is the writer. It:
  - checks consistency first (`CheckRuntimeInput::validate`): identity, generation, cwd, and that
    the credential is not the capability;
  - verifies the runtime directory (canonical, owner-only `0700`);
  - creates the file exclusively (`O_CREAT|O_EXCL|O_NOFOLLOW`, `0600`) and fsyncs the file and
    the directory.

  It never rewrites the file, so the same immutable binding is the input to every later recovery
  of that runtime, and a recovery cannot be rebound. A partial write is retained, and the runner
  refuses it. T23's production adapter must call this before spawning the host. Until then, the
  callers are the supervised test support (`crates/cli/tests/support/supervised_dispatch.rs`),
  which uses the production function.
- **Reader.** `runner/src/check-runner.ts::readCheckRuntime` reads the file
  (`protectedPath`, `O_NOFOLLOW`, ≤ 1 MiB, strict UTF-8). `validateCheckRuntime` then checks:
  - exact keys at every level, `version === 1`, and `validateLaunch`;
  - generation, assignment, profile revision and attempt agree;
  - bound cwd, configuration cwd and worktree are equal;
  - the four capability keys, and a credential that is not the capability.

  Any failure exits 1 with only `runner-check-input-invalid`, before ownership. Bootstrap and the
  profile module still re-validate the binding against Forge and the resources and
  authentication.

### One versioned contract (CONTRACT-2) and typed events (RUST-1)

- **Dead request variants removed.** `RunnerOperation` defines only `Admit` and `Observe`. The
  `Transcript`, `Recover`, `FollowUp` and `Retire` variants are removed; their real paths were
  already separate:
  - transcript: the bounded inspector wire;
  - recovery and follow-up: `Supervisor*` controls;
  - retirement and quiescence: host-consumed controls plus SIGTERM.

  The TypeScript validator rejects any other kind as `runner-request-invalid`. The module
  documentation of `crates/rpc/src/durable_execution.rs` and the header of `check-runner.ts` list
  the one contract.
- **New typed output.** `RunnerEvent` (`tag = "event"`, kebab-case, `deny_unknown_fields`, struct
  variants so even payload-free events reject extra fields) covers every runner emission:
  `owned` (typed `RunnerOwnerEvidence`), `authorized`, `recovery-prepared`, `bootstrapped`,
  `resumed`, `settled`, `runtime-failed`, `followup-accepted` and `followup-failed`.
  `RunnerOutput = Event | Response | Refusal`, and `RunnerOutput::parse` is the strict line
  parser (`runner-output-invalid`).
- **The daemon recovery driver parses events strictly.**
  `crates/server/src/durable_authorization/recovery.rs` parses every line into `RunnerEvent`.
  - It validates `owned.compatibility` against the claimed Launch component hash.
  - An unknown or malformed line is terminal for renewal: stop, `Blocked`, nothing inferred. It
    was previously ignored.
  - Follow-up bookkeeping reads typed fields.
- **New pipe sender.** `ControlPipe::request(&RunnerRequest)` sends admission and observation
  requests on the same fenced pipe. Success is transport only, and T23 must persist send intent
  first.

**Wire-compatibility assessment:**

- **Removed variants.** No Rust producer ever sent them, and the runner rejected all four. No
  store or journal persists `RunnerRequest`. Removing them changes no accepted behaviour, so
  protocol 1 is kept.
- **Changed rejection code.** For an undefined kind it changed from `runner-operation-unavailable`
  to `runner-request-invalid`. No Rust code matched on it.
- **Event shapes the daemon now rejects:**
  - the test-worker-only `admission` event (now a `RunnerResponse` line);
  - `settled {calls, tools}` (now `{status}`);
  - `recovered-model` (a fixture event, now written to a file).
- **Matched components.** Daemon and runner are matched by the component hash (T15).
  `componentHash()` now covers all sources, so every runner change yields a new hash. A retained
  Launch from an older build is refused as incompatible (fail closed).
- **No production runtime to migrate.** A retained runtime without `check-runtime.json` cannot be
  recovered by the new entry (`runner-check-input-invalid`, fail closed). Production durable
  dispatch was never enabled, so no such runtime exists in production.
- **Admission results.** The new `rejected` and `unknown` admission results use the existing
  `Admission` variants.

### Test workers are thin wrappers

Each worker swaps in only the model provider (a `ProviderSubstitution` behind the same
`guardProvider`). Everything else is the shipped path.

| Worker | Now |
| --- | --- |
| `runner/test/supervised-check-worker.mjs` | `runCheckRunner` + faux provider. The test barrier is awaited inside the first faux response (after bootstrap). The `recovered` crash boundary is fault injection by patching the pinned `SqliteStorage.prototype.commit` in the test process only; no seam exists in shipped code. |
| `runner/test/recovered-supervised-check-worker.mjs` | `runCheckRunner` + faux provider: recovery from the real `check-runtime.json` written at first admission. |
| `runner/test/check-bootstrap-tracer.mjs` | Execution branch calls the shipped `validateCheckRuntime` + `admitCheck` with a faux provider. |
| `runner/test/faux-check-entry.mjs` (new) | Positive control for the packaged-entry tests. |

The Rust support (`supervised_dispatch.rs`, `supervised_recovery.rs`) now:
- writes the input through `check_runtime::write`;
- sends admission as a typed `RunnerRequest` on the pipe;
- parses the `RunnerResponse` line;
- asserts that every runner line except the test-only `recovery-boundary` is a typed protocol-1
  value.

`calls`/`tools` counters no longer appear in runner events. The faux providers keep exact
scripted responses, and the tracer asserts `settled.status == "done"`, a committed answer and
Forge Outcome acceptance.

Remaining low-level fixtures are not Check compositions:
- `recovered-check-worker.mjs` and `recovery-control-worker.mjs` drive the shared
  `RecoveryConsumer` without bootstrap, to test host and daemon recovery mechanics;
- `session-worker.mjs`;
- the synthetic receiver.

`recovered-check-worker.mjs` no longer prints its non-protocol `recovered-model` line;
`recovery_crash.rs` reads the model count from its file instead.

## Acceptance checks

| Check | Evidence |
| --- | --- |
| Packaged entry refuses execution without successful bootstrap, a valid profile, model authentication and a lease | `runner/test/check-entry.test.mjs` runs `src/check-entry.ts` **unsubstituted** under the real `forge-runner-host`. **No lease:** admit → `assignment-lease-denied`, no bootstrap call, no store. **Failed bootstrap, missing model key, incompatible profile:** each gives `rejected/assignment-runtime-failed`, `runtime-failed`, no `bootstrapped`, no store, and a retry is fenced. **Invalid input:** seven cases exit 1 with `runner-check-input-invalid` before ownership. A positive control with the identical fixture and only a faux provider bootstraps, admits and settles `done`, so each refusal differs by exactly one factor. |
| Tests use it with only the provider substituted | The workers above; `durable_supervised_tracer` (10 scenarios) and `durable_check_bootstrap` (5 tests) run the shipped modules. |
| Recovery input written by named production code, validated strictly, real-process recovery test runs the shipped recovery entry from its real input file | `forge_runner_host::check_runtime::write` (unit tests `check_runtime_tests.rs`: owner-only, exclusive, immutable, strict round-trip, consistency and directory refusal). The `recovered` supervised scenario: first admission through the shipped entry, crash before the first generation commit, then the host spawns `recovered-supervised-check-worker.mjs` (`runCheckRunner`). It reads the same `check-runtime.json` (asserted present, never rewritten), re-bootstraps, resumes the original admission and root, and the Outcome is accepted. |
| One versioned contract, no defined-but-rejected operations | `rpc` tests `protocol_one_defines_no_rejected_request_operations` and `runner_events_are_typed_strict_and_cover_every_runner_emission`; `process.test.mjs` asserts an undefined kind is `runner-request-invalid`. |
| Existing AC3/AC8/AC17/AC18 runner tests pass against the production entry, including secret-canary scans | All 10 supervised scenarios and 5 `durable_check_bootstrap` tests now run `check-runner.ts`. Their canary scans are unchanged: capability secret, `T07-SUPERVISED-MODEL-KEY`, behavior, and runtime storage in `durable_check_bootstrap`. `check-entry.test.mjs` adds key, capability and behavior canary scans of the unsubstituted entry's stdout and stderr. The Node bootstrap, profile and authorization suites are unchanged and pass. |
| Synthetic-only code no longer reachable as the packaged entry | `ENTRY_RELATIVE` changed. Forgeup test `packaged_entry_is_the_production_check_runner_not_the_synthetic_fixture`. The Node test rejects any `faux` reference in `runner/src/`. `package-bundle.mjs` bundles only `src/`. |

## Gates

`PATH` had the real Node 24.18.0 first (`~/.volta/tools/image/node/24.18.0/bin`); the Volta shim
breaks `durable_check_bootstrap`, as recorded in the T10 evidence.

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 895 passed, 0 failed, 1 ignored (pre-existing opt-in probe). All 10 supervised tracer scenarios passed. |
| `git diff --check` | pass |
| `runner`: `npm run check` (tsc) | pass |
| `runner`: `npm test` | pass: 97/97 (90 before, plus 7 in `check-entry.test.mjs`) |

## Not claimed

- **No production launch.** The production adapter still refuses durable dispatch. Nothing in
  `server` calls `check_runtime::write` or `ControlPipe::request` yet (T23).
- **`forge-runner-host` is still not bundled** (T24).
- **No real-provider (class c) test exists.** The unsubstituted entry is exercised only up to the
  point before a model request.
- **The `recovered` scenario still crashes before the first generation commit.** An in-flight
  model request remains `unanswered` on recovery (unchanged T10 behaviour).
- **The host-side recovery receiver tests still use the synthetic receiver fixture.** They test
  host and daemon handoff, not the Check composition.
