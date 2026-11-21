# T18 — Independent implementation and coding-standards review

- **Date:** 2026-11-20
- **Reviewed snapshot:** `1fc65a57667093ceefdc737e8278ecb6634c58b2` (T17 head)
- **Reviewed diff:** `git diff 4870692..1fc65a5` (T02 `f1fb797` through T17; 334 files, +50 017 / −188). `4870692` is the last pre-T02 commit. T01 research is included only as cited context.
- **Inputs:** parent Spec `.scratch/pi-durable-task-execution-spec.md` (AC1–23, Implementation Decisions, Domain changes), Task bodies T02–T17, `CODING_STANDARDS.md`, per-Task evidence `docs/research/pi-durable-t03..t17/`. The evidence was treated as claims to check, not as proof.
- **Method:** `review-implementation` and `review-coding-standards`. Five read-only reviewers, each starting with fresh context, covered one area each:
  - admission, ownership and protocol;
  - leases, authentication and profiles;
  - effects, retirement and recovery;
  - operator surfaces and migration;
  - dispatch, packaging, rollout and documentation.

  Every BLOCKER and the most serious MAJOR were then rechecked by hand against the code, at the file:line references below. No code, configuration, profile, dispatch or rollout state was changed.

## Verdict

**Not ready for release verification. T19 must not start.** Two blockers stand, plus major findings that need fixing or an explicit operator decision.

Nothing found in the existing safety logic is unsafe. Every path examined fails closed:
- exclusive ownership is taken before any writable open;
- retries return the original result exactly;
- revocation happens before any signal;
- uncertain effects and capacity are retained;
- inspection is read-only.

The blockers are about completeness and contract: a criterion cannot be met or verified with the code as it stands.

## Gates run on the reviewed snapshot (this review)

`PATH` had a real Node 24.18.0 first; Volta's shim breaks `durable_check_bootstrap`, as recorded in the T10 evidence.

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 881 passed, 0 failed, 1 ignored (pre-existing opt-in `pi_flags_runtime_selected_and_empty_resources`). All 10 supervised tracer scenarios passed. |
| `git diff --check 4870692..1fc65a5` | pass |
| `runner`: `npm run check` (tsc) | pass |
| `runner`: `npm test` | pass: 90/90 |

These are local deterministic gates. They do not show release or recovery readiness (VERIFY-2).

## Coverage classes

Every test is labelled with one class:
- **(a)** mock, fake or synthetic, including in-process SQLite with fake transports and scripted sockets;
- **(b)** real process with a fake model: real Node runner, `forge-runner-host`, flock, subreaper, SQLite, Git and CLI, but a faux provider;
- **(c)** real provider.

**The diff contains no class (c) test.**

Also, every (b) execution test runs a **test-only composition**, not shipped code (see B1):
- `crates/cli/tests/support/supervised_dispatch.rs`;
- the workers `runner/test/supervised-check-worker.mjs`, `recovered-supervised-check-worker.mjs` and `check-bootstrap-tracer.mjs`.

## Findings

Severity:
- **BLOCKER:** must be corrected, and the corrected snapshot reviewed again, before T19.
- **MAJOR:** must be corrected or explicitly accepted by the operator before T20.
- **MINOR:** should be fixed; does not block on its own.

### B1 — BLOCKER: no production durable launch or execution path exists

AC1, AC3, AC4, AC5, AC19 and AC22. Rules VERIFY-2 and TEST-1.

Spec, AC1: *"An eligible Task with durable-compatible Check pins provisions a Forge-owned runner directly using pi-durable."* AC22 asks for a real-provider cycle on the tested Forge Revision.

Verified:
- Production composition (`crates/server/src/lib.rs:930-948`) is `ReleaseGate`, then `retirement::Factory`, with `recovery: None`. For `pi-durable`, `Factory::validate_profile` and `validate_preflight` refuse (`crates/server/src/durable_authorization/retirement.rs:243-255`). `Adapter::probe`, `provision` and `observe` all return "durable production dispatch and recovery remain disabled" (`retirement.rs:280-292`). Setting the gate to `Controlled` still cannot launch anything.
- Launch orchestration exists only in test support (`crates/cli/tests/support/supervised_dispatch.rs:420-479`):
  - reservation and identity minting;
  - runtime directory preparation;
  - `durable_profiles::prepare`;
  - supervisor spawn and the lease driver.

  That provision blocks until the run settles, uses `forge_revision: "controlled-source-tracer"`, and spawns the test executable as `--supervisor`.
- `durable_execution::Transport` has no non-test implementation (only `store/src/durable_execution_tests.rs`).
- The packaged entry is `ENTRY_RELATIVE = "runner/src/main.ts"` (`crates/forgeup/src/runner.rs:24`). That file starts *"T04 controlled synthetic runner. No real providers, tools or production launch."* It uses a hard-coded faux provider and has no bootstrap or profile.
- `runner/src/check-recovery.ts` is not run by any test, and its input `check-runtime.json` is written by no code. Only `runner-host/src/purge.rs` names it. Yet `docs/research/pi-durable-t10/completion.md` cites it as the writable Check runner.
- The `forge-runner-host` binary is not part of the runner bundle or the Installation.

T16 and T17 disclose this as "production durable dispatch remains disabled". However:
- T16 is marked complete as "Gate complete scheduling";
- the plan has no remaining code Task (T18–T20 are non-code);
- T19 requires "packaged integration … real-provider handoff".

The plan therefore cannot reach AC22 as written.

**Consequence:**
- AC1, AC3, AC4, AC5 and AC18 are shown only on test compositions.
- The gate has nothing to enable.
- T19 cannot run on a production Revision.

**Required remediation:**
1. Implement the production durable adapter in `server`:
   - identity minting;
   - an admission `Transport` over the supervisor pipe;
   - capability delivery;
   - a non-blocking provision/observe;
   - recovery wiring behind the gate.
2. Write a production runner entry that composes bootstrap, profile and model auth, plus a real Check runtime. Point `ENTRY_RELATIVE` at it.
3. Package and attest `forge-runner-host`.
4. Make `check-recovery.ts` and its `check-runtime.json` contract real and tested, or remove them.
5. Rerun the AC1/3–6/8/18 suites against that artifact.
6. Correct the T10 evidence statement.

**Ownership is undecided.** The plan says *"no … extra plan branches or silent additions are authorized"*, and none of T02–T17 explicitly owns the production adapter. The closest owners are T07, T15 and T16. The operator must assign this correction, or re-scope the Spec, before work continues.

### B2 — BLOCKER: explicit runtime deletion fails for every really retired or crash-reconciled runtime

AC16. Owner T14. Rule TEST-1.

Spec, AC16 Verification: *"… then delete eligible runtime storage."*

Verified:
- `recognized()` rejects any file outside the nine-name `REMOVAL_ORDER` (`crates/runner-host/src/purge.rs:13-23,45-46`).
- Ordinary lifecycle writes, and never removes, files outside that list. For example, `retire()` writes `retirement-intent.json` through `stop_owned` (`crates/runner-host/src/retirement.rs:79,101`). Others are:
  - `recovery-crash-<seq>.json` (`crash.rs`);
  - `recovery-quiescence-{intent,receipt}.json` (`quiescence.rs`);
  - `recovery-proof-*.json` (`recovery_proof.rs`);
  - `owner.<pid>.pending` (`supervisor.rs`).
- The only (b) purge test, `runner-host/tests/authorization_session/purge_runtime.rs:79-102`, makes the store eligible with an SQL-inserted retirement row, so these files never appear.

**Consequence:** every Assignment retired through the real path fails with `runtime-purge-storage-unsafe`. This fails closed, so nothing unsafe happens, but AC16 deletion cannot be used.

**Remediation:**
- Recognize the lifecycle artifacts with validated exact names or patterns.
- Add composed tests for real `retire_agent_assignment` → reconcile → purge and for the crash-reconciliation path.
- Add a CLI/RPC end-to-end purge test (see M6).

### M1 — MAJOR: no reconciliation path after lease expiry or daemon loss

AC8, AC11, AC12 and AC23. Owners T09 and T10. Rules SAFE-2 and SAFE-3.

Verified:
- On pipe EOF or lease expiry, the host watchdog does the following (`crates/runner-host/src/watchdog.rs:415-478`):
  - writes `watchdog.json`;
  - sends TERM, then KILL;
  - keeps the flock forever.
- `reconcile_crash` gives up whenever `watchdog.json` or `retirement-intent.json` exists (`crates/server/src/durable_authorization/retirement.rs:102-106`).
- Retirement needs the lost pipe. `stop_owned` would also fail `worker.verify()` once the leader has been killed.
- `resolve_unknown_agent_resources` retires only `provision-intent`.

**Consequence:** any daemon outage longer than the 30 s lease leaves the Assignment holding capacity permanently, with no supported way out. This is safe under SAFE-2, but `docs/durable-execution.md:268` tells operators to *"plan for an Assignment to need recovery or retirement after any daemon outage"*, and no such path exists. The same applies (reported, not run) to a daemon restart between `begin_retirement` and pipe delivery.

**Remediation:** add a watchdog-fenced reconciliation. For example:
- the original subreaper writes an ECHILD receipt after its watchdog KILL;
- Forge accepts that receipt under exclusive inspection.

Alternatively, add an explicit, audited operator override for durable runtime resources. Test daemon death during execution, and correct the documentation.

### M2 — MAJOR: documentation states behaviour that does not exist

AC23. Owner T17. Rule VERIFY-2.

| Location | Claim | Reality |
| --- | --- | --- |
| `docs/durable-execution.md:3-4` | "Forge can run …" | Present tense; no production execution exists (B1). |
| `docs/durable-execution.md:44` | runner "spawned by the daemon" | Same. |
| `docs/durable-execution.md:263-270` | "the daemon renews …" | Same. |
| `docs/durable-execution.md:15-20` | Implies only release-gate eligibility is missing | B1, B2 and M1 are also missing. |
| `runner/README.md:5-6` and `docs/dispatch.md:517-520` | Same daemon-spawn claim | Same. |
| `docs/durable-execution.md:105-107` | The model key never reaches transcripts | Bash tools run as the same user. A tool that reads the key file stores it unredacted in runtime SQLite and transcripts, and sends it to the provider. `redact()` (`runner/src/profiles.ts:103-124`) covers provider output only. |

**Remediation:**
- State that the production adapter is absent and that these behaviours are verified only in controlled fixtures.
- Qualify the key-exposure claim to match the trusted-local-host section (`:328-341`), or also redact tool results.

### M3 — MAJOR: AC19 met only in part

Owner T15. Spec: *"Preflight rejects missing or incompatible components."*

- `durable_profiles.rs:57-76` validates only the profile and the key file; it does not check runner components.
- The Launch component hash is not mapped to the bundle tree hash. The effect inspector must match the Launch hash (`crates/server/src/durable_authorization/operator.rs:15`). After a Revision switch, retirement and effect resolution for retained Assignments are therefore blocked, and `runner resolve` cannot find them by component hash.
- `docs/development.md` omits this.

**Remediation:** add a component preflight, map the identities, and add a Revision-switch retained-retirement test.

### M4 — MAJOR: a Forgeup-managed daemon has no model-credential mechanism

AC18 and AC19. This confirms the T17 finding.

- `durable_model_key_file: None` is set at `crates/server/src/lib.rs:220`.
- Forgeup strips `FORGE_*` variables (`crates/forgeup/src/lib.rs:903-905`).

This is documented honestly. Still, the only supported install path cannot run durable work. Close it, or explicitly limit the Spec to manual daemons, before T20.

### M5 — MAJOR: the runner wire protocol has drifted

AC5. Rule CONTRACT-2.

- Wire v1 defines `Transcript`, `Recover`, `FollowUp` and `Retire` (`crates/rpc/src/durable_execution.rs:134-158`).
- The runner rejects all four as `runner-operation-unavailable` (`runner/src/contract.ts:413-414`).
- The real paths are separate `host_*` controls, and the bootstrap-gated test worker reports admission as an untyped event.
- Runner events are matched as untyped `serde_json::Value` strings (`crates/server/src/durable_authorization/recovery.rs:299,429-436,482-521`), which also breaks RUST-1.

**Remediation:** keep one versioned contract. Either remove the dead variants or make them the real path, with typed event enums that use `deny_unknown_fields`.

### M6 — MAJOR: contract and fault tests missing for new operator surfaces

AC15 and AC16. Rules CONTRACT-2 and TEST-2.

- `assignmentPurgeRuntime` and `assignment-purge-runtime` have only compile and help checks; T14 admits this.
- A follow-up delivery marked `unknown` is re-sent only in a later recovery session (`recovery.rs:384-417`). No daemon-level test covers an uncertain send or reply followed by a cross-session resend that produces exactly one admission. This is AC15's first Verification item; only the Node runner test covers it.

### Minor findings

1. **Revoked runner window** (AC11).
   - Closing the Assignment does not fence the runner's local lease.
   - Admissions remain possible until host TERM, which in the recovery path can be up to about 10 s (`recovery.rs:199`).
   - The window is bounded by the lease. Fence it over the pipe first, or document it.
2. **AC12 dispatch rejection tested only for effects** (TEST-2).
   - `retirement_tests.rs:70` asserts `capacity_reserved()` for launch, ownership and retirement uncertainty, but never attempts `claim_next_agent_assignment` after a restart.
3. **Unknown admission is never re-sent** (SAFE-2).
   - A dropped request or a crash between `harness.root()` and `persistRoot` (`runner/src/main.ts:138-139`) stays `unknown` permanently (`app/src/durable_execution.rs:179-184`).
   - Safe, but no reconciliation path is documented or tested besides retirement.
4. **RUST-3: discarded follow-up error.**
   - `let _ = repository.mark_followup(...)` (`recovery.rs:438`) discards a store error without a diagnostic.
5. **RUST-3: supervisor error handling.**
   - In `supervisor.rs:141-150`, `child.kill()?` returns before `wait()`.
   - `persist_owner` overwrites the previous `owner.json` on restart (whether that evidence is kept elsewhere is unverified).
6. **RUST-1: stringly-typed owner evidence.**
   - Owner evidence dev/ino are compared as strings (`runner-host/src/lib.rs:31-38`, `supervision.rs:181-182`).
7. **CONTRACT-1: schema revision 23 reused.**
   - The migration and purge tables were added to revision 23 after `5dc8ee5` had already stamped 23.
   - A store created at that commit passes `validate` and then fails at runtime.
   - Acceptable only if no 23 store was ever distributed (unverified). Otherwise bump the revision. The comment in `schema_compatibility.rs:19-20` is stale either way.
8. **CONTRACT-2: changed field meaning.**
   - `list_record_checks`/`checks::load` `profile_revision` now reports the *effective* pin.
   - This changes the meaning of an existing field with no compatibility note.
9. **CONTRACT-3: management protocol doc not updated.**
   - `crates/docs/management-protocol.md:60-79` does not list the managed-only `--durable-node` and `--durable-effect-inspector` launch inputs.
   - Keeping protocol revision 1 is justified only in the T15 evidence.
10. **SEC-2 (spirit): key-file path inherited by child processes.**
    - The daemon does not strip `FORGE_DURABLE_OPENAI_KEY_FILE` from the environment inherited by Project Setup Hooks, `git` and `gh`.
    - Only the path is exposed, not the key.
11. **AC18: env-var configuration path untested.**
    - The documented `FORGE_DURABLE_OPENAI_KEY_FILE` route (`server/src/lib.rs:177`) has no test; tests only use the builder.
12. **Status listing.**
    - Human status always prints "delivery not proven" and omits the Receipt's `delivery`/`admission_id` (`crates/cli/src/assignment_inspection.rs:191`).
    - Durable status reads four sources outside a single snapshot (`server/src/lib.rs` ~1250-1277).
13. **Purge audit.**
    - `AlreadyAbsent` is journaled as `purged`, so the audit cannot tell real deletion from storage that was never present.
14. **Test name overstates coverage.**
    - In `rollout_tests.rs:257` (`…keep_spec_and_dependency_gates…`), both Tasks are refused by the Spec gate. The dependency gate is never exercised.
15. **RPC doc comment misplaced.**
    - The `assignmentFollowUp` comment sits on `assignmentPurgeRuntime` (`crates/rpc/src/lib.rs:202-205`).
16. **Registration side effect.**
    - Registering a durable profile selects it as the Workspace Task profile, which makes readiness `task-profile-invalid` and blocks Herdr Grilling.
    - Documented with a workaround (`durable-execution.md:171-175`), but still an operator hazard.
17. **Unverified items.**
    - The bundle build runs `npm ci` from `PATH` (`runner/package-bundle.mjs:35`). Lockfile integrity mitigates this.
    - A read-only WAL connection may create `-shm` before the host lock (`runner/src/storage.ts:37`).
    - Transcript cursor stability assumes monotonically committed entry IDs.

## Acceptance-criteria trace

"Missing" compares each criterion with its Spec *Verification* line. Every criterion lacks class (c) coverage, and every criterion is affected by B1, because no production path exists.

| AC | Main implementation | Tests (class) | Missing / blocking |
| --- | --- | --- | --- |
| AC1 headless dispatch | `retirement::Factory`/`ReleaseGate` (refuses); test-only `supervised_dispatch.rs` | supervised tracer, 10 scenarios (b; Herdr calls panic, no dispatch workspace); `provider_wiring_tests` (a) | **B1**: no production adapter |
| AC2 Forge authority | `app/src/durable_execution/completion.rs`, tracer `gates()` | tracer missing/rejected (b); `rollout_tests` (a) | Stack/publication with a durable pin not run; dependency gate not run in production composition (minor 14) |
| AC3 bootstrap / Outcomes / secrets | `runner/src/bootstrap.ts`, `check-runtime.ts`, `assignment_delivery.rs`, `env_clear` spawn | `bootstrap.test.mjs` (a); `durable_check_bootstrap` (b, canary scans) | **B1** (shipped entry has no bootstrap); M2 key-exposure wording |
| AC4 per-Assignment isolation | UNIQUE and immutability triggers in `durable_execution.sql`; SHA-named runtime directory | `process.test.mjs` concurrent/crash isolation (b, synthetic `main.ts`); continuation chain (b; identity asserts compare harness-built IDs) | **B1**; no concurrent production dispatch; no (c) Implementation→Review |
| AC5 versioned idempotent protocol | `app/src/durable_execution.rs:83-186`, `rpc/src/durable_execution.rs`, `runner/src/contract.ts` | store exact/conflict/dropped-reply/interruption (a); `process.test.mjs` SIGKILL/partial intent (b) | **B1** (no production transport); M5; minor 3 |
| AC6 ownership before open | `runner-host/src/supervisor.rs:91-139`, `supervision.rs`, `inspection.rs` | `process.test.mjs` concurrent/stale-PID (b); `authority.rs` (b); concurrent recovery claims (a) | Recovery/reconnect race only at store level; PID reuse simulated by tampering; no writable-before-flock path found |
| AC7 same-Assignment recovery | `store/.../recovery.rs:41-253`, `server/.../recovery.rs:111-166`, `runner/src/resume-runtime.ts:187-209` | `recovery_tests` (a); `recovery_{crash,reconciliation,handoff,progress}` (b) | Production recovery is `None` (B1); `check-recovery.ts` never run |
| AC8 leases | `runner/src/authorization.ts` (provider and every tool guarded; retries and compaction off), `watchdog.rs`, `authorization_lease/session.rs` | fake-clock (a); real 30 s outage model/child (b); watchdog with JS stopped (b) | **M1**; no (c) stream expiry |
| AC9 interrupted effects | `runner/src/interrupted-effects.ts:209-287`, `app/.../effect_resolution.rs:67-85` | real bash + HTTP endpoint, SIGKILL, `mutations == 1` (b); `effect_admission`, `effect_resolution` (b); store (a) | Adequate deterministically; after an effect only retirement is possible (documented) |
| AC10 no destructive repair | `server/.../recovery_worktree.rs` (read-only Git; no reset/clean/stash/checkout anywhere in the diff) | `retirement.rs:161` tracked/staged/untracked preservation (b); `recovery_progress` (b) | — |
| AC11 revoke before cleanup | `app/src/lib.rs:3131-3150`, `store/.../retirement.rs:17-29`, `server/.../retirement.rs:159-200`, `runner-host/src/retirement.rs:93-131` | ordering, delayed, escaped child, write failure, mismatch, missing supervisor, controller restart (b) | M1; minor 1 |
| AC12 retained blockers/capacity | `store/src/assignment_retirement.rs`, `reconcile_retirement` | `retirement_tests`, `supervised_retirement_tests` (a); `retirement.rs:433,500` (b) | Minor 2; M1 |
| AC13 missing Outcome | `recovery.rs:500-516`, `app/.../status.rs` | `durable_check_bootstrap:164,169` (b); store (a) | Retirement in that test is SQL-controlled; no explicit no-Continuation assertion |
| AC14 read-only inspection | `rpc/.../assignment_inspection.rs`, `cli/.../assignment_inspection*`, `runner/src/storage.ts:61-145` | CLI→daemon→Node reader (b); `transcript_status` live/killed/closed (b); live-append pagination (b) | Crashed state not exercised through RPC alone; minor 17 |
| AC15 follow-ups | `store/.../followup.rs:129-178` (one IMMEDIATE transaction), `recovery.rs:384-446`, `runner/src/followup.ts` | store concurrency/reopen/blocking (a); daemon delivers once (b); Node idempotence (b) | **M6** (cross-session unknown resend at daemon level) |
| AC16 retention/deletion | `runner-host/src/purge.rs`, `store/.../purge.rs`, owner-only `storage.rs` | host/store units (a); `purge_runtime` (b, synthetic eligibility) | **B2**; M6; no post-retirement permission check |
| AC17 explicit profiles | `runner/src/profiles.ts`, `server/src/durable_profiles.rs`, `app/src/durable_profiles.rs` | durable_profiles tests (a/b); ambient HOME/settings/AGENTS.md canary (b); retirement after source removal (b) | Ambient test checks composition, not a full run; inspection after source loss implicit |
| AC18 provider auth | One mechanism: OpenAI key file (`runner-host/src/model_auth.rs`, `profiles.ts:50-71`) | mocked `fetch` Bearer/single request/redaction (a); file-safety negatives (a) | **No real request ever made (c)**; M4; minor 10–11 |
| AC19 matched components | `forgeup/src/runner.rs`, `runner/package-bundle.mjs` | 6 synthetic-bundle tests (a); T15 manual packaged run (not retained) | **B1** (synthetic entry, no host binary); **M3** |
| AC20 pin migration | `store/src/check_profile_migration.rs:105-207` (one transaction; append-only triggers; original pin untouched) | store (a); CLI→daemon→store (b) | Rejection of uncertain (non-active) and other-Task/Workspace cases untested; single-Check fixture (unrelated Check unchanged untested); invalidation of a real passed Assessment untested |
| AC21 rollout/rollback | `release_gate.rs`, persisted sticky rollback, unknown backend fails closed (`herdr.rs:127`) | release_gate (a); `rollout_tests` (a, existing durable Assignment inserted directly); tracer `rollback` (b) | Cannot change defaults while B1 stands; minor 14 |
| AC22 release gate | — | Deterministic coverage exists for each listed fault class (a/b; daemon "crash" is in-process abort, not real process death) | **Unrun:** real-provider Implementation→Review; packaged dispatch; any Forgeup-managed launch. Blocked by B1. |
| AC23 honest docs | `docs/durable-execution.md` (trusted-local-host, no exactly-once/process-death at `:328-341`) | guide examples parsed by real clap parser | **M2** |

## Domain reconciliation

| Proposed change | Applied | Result |
| --- | --- | --- |
| Assignment Authorization Lease | `crates/CONTEXT.md:227` | Matches |
| Assignment Execution Recovery | `crates/CONTEXT.md:231` | Matches |
| Forge Workspace | `crates/CONTEXT.md:64` | Matches |
| Check | `crates/CONTEXT.md:172` | Same meaning; keeps existing Manual/Projectless clauses |
| Agent Profile | `crates/CONTEXT.md:244` | Matches, plus existing Skill/freezing clauses |
| Forge Dispatch Workspace | `crates/CONTEXT.md:252` | Matches; keeps an existing ownership clause |
| Agent Assignment (not proposed) | `crates/CONTEXT.md:212` | "Effective revision at reservation"; consistent with migration |
| ADR headless (proposed 0012) | `0014-headless-durable-task-execution.md` | Verbatim, plus a renumbering note and a status paragraph |
| ADR recovery (proposed 0013) | `0012-authorized-durable-recovery-and-conservative-retirement.md` | **Differs:** retitled and body rewritten with implementation detail and a temporal status ("remain disabled") that will go stale. Substance kept. Minor; restore the decision text and move status into docs. |
| ADR migration (proposed 0014) | `0013-explicit-backend-and-check-profile-migration.md` | Verbatim, plus a representation paragraph |
| ADR-0005/0007/0008 cross-references | Those ADRs | 0005 and 0008 point to 0014; 0007 notes reuse |
| Task, Continuation and Assignment Session unchanged; "execution tasks" qualified | Guide and ADR-0014 | Matches |

## Standards confirmed in the reviewed code

- **TX-1/TX-2:** reservation, send, result, follow-up gate, migration eligibility and purge journal each run in one IMMEDIATE transaction.
- **TX-3:** scope is authorized before replay; exact retries return the stored result; conflicts are rejected.
- **TX-4:** transport, Git and process calls run with no transaction held.
- **SAFE-3:** no path clears holds, resets budgets or releases capacity as a side effect.
- **SAFE-4:** purge paths are canonicalized and no-follow, with owner and nlink checks.
- **SEC-3:** explicit argument arrays, no shell.
- **RUST-2:** no unjustified production `unwrap`/`expect` in scope.
- **AC6:** no writable runtime open happens before the host flock.

## Required next steps

1. **Operator decision on B1:**
   - which deliverable owns the production adapter, runner entry and runner-host packaging (T07, T15 or T16 by intent);
   - or an explicit Spec re-scope.

   The plan forbids silently adding Tasks.
2. Correct B2 (T14), M1 (T09/T10), M2 (T17), M3 (T15), M5/M6 (T12/T14) and the applicable minor items. Resolve M4, or limit the Spec explicitly.
3. Re-review the corrected snapshot, and record its revision here. T19 consumes only that reviewed snapshot.

**T18 status:** the review deliverable is complete, but it records blockers. T18 stays unchecked in the plan until the corrected snapshot has been reviewed again with no unresolved BLOCKER.
