# T18 — Independent implementation and coding-standards review

This report has two passes. The **second pass** (below) is current. The
[first pass](#first-pass--2026-11-20-snapshot-1fc65a5) is kept unchanged as history.

## Second pass — 2026-11-21, snapshot `646abd9`

- **Date:** 2026-11-21
- **Reviewed snapshot:** `646abd98143e4f5351f71eb9a4745f0729a91956` (T24 head)
- **Reviewed diff:**
  - `git diff 1fc65a5..646abd9`: the corrective Tasks T21 `b70fcc9`, T22 `f39417b`, T23 `5f44eda` and T24 `646abd9`, plus the plan commit `f60b880`. 89 files, +8 068 / −1 503.
  - Every first-pass finding was re-located and rechecked in the full T02–T24 range `4870692..646abd9` (355 files, +56 583 / −189).
- **Inputs:**
  - the parent Spec;
  - Task bodies T18 and T21–T24;
  - `CODING_STANDARDS.md`;
  - the evidence in `docs/research/pi-durable-t21..t24/`, treated as claims to check, not as proof.
- **Method:** `review-implementation` and `review-coding-standards`.
  - Three read-only reviewers, each starting from fresh context, covered one area each:
    - the production launch path, protocol and packaging (B1, M3, M4, M5);
    - purge and follow-ups (B2, M6, minors 7 and 13);
    - reconciliation, documentation and the remaining minors (M1, M2, minors 1–17).
  - Each new BLOCKER and MAJOR was then rechecked by hand at the file:line references below.
  - No code, configuration, profile, dispatch or rollout state was changed.

### Verdict (second pass)

**Not ready for release verification. T19 must not start.**

- **Resolved:** the two first-pass blockers. B1 is resolved structurally: a production launch path now exists, behind the default-withheld gate. B2 is fully resolved. M5 is resolved, M3 mostly, and minors 7, 9 and 13 are resolved.
- **New blockers:** making launch real turned two earlier liveness gaps into reachable contract failures:
  - **B3:** any daemon restart permanently strands live durable Assignments. This is M1, escalated.
  - **B4:** a launch that fails after its claim can never be retired.

  Both fail closed: no unsafe execution, release or replay was found. But they leave production-launched Assignments with neither a recovery path nor a retirement path. That breaks AC7, AC11 and AC17, and the Spec's Implementation Decision to *"keep runtime storage and ownership recoverable across daemon/runner restart and compatible Revision operations"*. T19's required "daemon/runner restart ordering" and "incomplete retirement" scenarios cannot pass.

### Gates run on `646abd9` (this review)

`PATH` had a real Node 24.18.0 first.

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 908 passed, 0 failed, 1 ignored (pre-existing opt-in probe) |
| `durable_supervised_tracer` (rerun separately) | pass: all 11 scenarios, including `T24 packaged Revision dispatch and switch` |
| `git diff --check 1fc65a5..646abd9` | pass |
| `runner`: `npm run check` / `npm test` | pass / 97 of 97 |

These are local deterministic gates, not release evidence.

### Coverage classes (second pass)

The classes are the same as in the first pass:
- **(a)** mock or fake;
- **(b)** real process, faux model;
- **(c)** real provider.

**There is still no class (c) test.**

What changed since the first pass:
- The tracer now runs the production composition (`launch::compose`). The only test-only parts are:
  - the gate setting (`enable_controlled`, called only from `durable_supervised_tracer.rs`);
  - a faux-provider entry;
  - two bootstrap fault injections.
- The non-packaged scenarios run the test binary as `--supervisor`. Only the `packaged` scenario runs the real `forge-runner-host`.
- `runner/package-bundle.mjs` itself is run by no test. `supervised_packaged.rs:125-175` rebuilds the bundle layout and manifest independently.
- `check-entry.test.mjs` stops every case before any model request, so the shipped entry's real-provider path has never run.

### First-pass findings: status at `646abd9`

| Finding | Status | Evidence |
| --- | --- | --- |
| **B1** no production path | **Resolved structurally; not yet reachable** | See the B1 notes below. |
| **B2** purge after real retirement | **Resolved** | See the B2 notes below. |
| **M1** no reconciliation after lease expiry / daemon loss | **Open → escalated to B3** | See B3. |
| **M2** docs over-claim | **Partial** | Main guide updated; see M2′ below. |
| **M3** AC19 component preflight / hash mapping | **Mostly resolved** | See the M3 notes below. |
| **M4** managed daemon has no model credential | **Open, acknowledged** | `crates/server/src/lib.rs:273` (`durable_model_key_file: None`); T24 README. |
| **M5** wire drift | **Resolved** | See the M5 notes below. |
| **M6** purge/follow-up contract tests | **Purge resolved; follow-up open** | See the M6 notes below. |
| Minor 1 revoked runner window | Open | `launch.rs:887-900` `fence`/stop sends no revocation to the host. The last grant (≤ 30 s) stays usable. |
| Minor 2 AC12 dispatch after restart untested | Open | `store/src/durable_execution/retirement_tests.rs` unchanged. |
| Minor 3 unknown admission never re-sent | Partial | Unknown follow-ups are re-sent across sessions (`recovery.rs:513-543`). An unknown launch admission fences and is not retried (`launch.rs:1018`). |
| Minor 4 `let _ = mark_followup` | Open, now also on the launch path | `recovery.rs:555`; reached from the launch driver too. |
| Minor 5 `kill()?` before `wait()`; `owner.json` overwrite | Open | `runner-host/src/supervisor.rs:65,141-150` |
| Minor 6 dev/ino as strings | Open | `runner-host/src/supervision.rs:132-133,182` |
| Minor 7 schema 23 reuse | Resolved going forward | Revision 24 is new (`schema_compatibility.rs:20-21`, `upgrade.rs:58-60`); 23→24 upgrade tests at `upgrade_tests.rs:415-438`. |
| Minor 8 `profile_revision` meaning | Open | No compatibility note (`docs/records.md:731`). |
| Minor 9 management protocol doc | **Resolved** | `crates/docs/management-protocol.md:93-95` |
| Minor 10 key-file env var inherited | Open | `server/src/lib.rs:196`. The host spawn uses `env_clear` (`launch.rs:609`), but hooks, `git` and `gh` still inherit it. |
| Minor 11 env-var key path untested | Open | No test sets `FORGE_DURABLE_OPENAI_KEY_FILE`. |
| Minor 12 "delivery not proven" / non-snapshot status | Open, worse | `Receipt.delivery` now exists (`followup.rs:81`), but `cli/src/assignment_inspection.rs:204` ignores it. There are still four reads (`server/src/lib.rs:1313-1324`). |
| Minor 13 `AlreadyAbsent` journaled as purged | Resolved | Append-only `durable_runtime_purge_removals` (schema 24). |
| Minor 14 rollout test name | Open | `rollout_tests.rs:257-298` |
| Minor 15 misplaced RPC comment | Open | `rpc/src/lib.rs:202-206` |
| Minor 16 durable registration selects Task profile | Open (documented) | `agent-profile-policy.md:151-153` |
| Minor 17 npm from PATH; `-shm` before lock | Open | `package-bundle.mjs:48`; `runner/src/storage.ts:37` |
| ADR-0012 temporal status | Open, now wrong | ADR-0012 line 5 says recovery is disabled; see M2′. |

**B1 notes.**
- **Composition:** `application_from_config` (`server/src/lib.rs:986-1002`) wraps `launch::compose` (`launch.rs:1270`) in `ReleaseGate`. Managed daemons get components from `--durable-runner-bundle`.
- **Provision is non-blocking** (`launch.rs:509-640`). The order is:
  1. preflight;
  2. the atomic `begin_agent_provision` claim;
  3. `reserve`;
  4. the protected runtime directory and `check-runtime.json`;
  5. `begin_agent_launch`;
  6. the `env_clear` host spawn;
  7. the driver thread.
- **Driver:** waits for typed `owned` evidence, then `bind_inspected` and the grant, then admission over the production `PipeTransport`. Settled answers are inspected, and a stop hands the original Child and pipe to retirement.
- **Packaging:** `forge-runner-host` is packaged (`package-bundle.mjs:41`) and verified by Forgeup (`forgeup/src/runner.rs:230`).
- **Not yet reachable:**
  - `LaunchControl::default()` withholds launches (`lib.rs:988`), by design until T20.
  - No shipped command can grant `Controlled`.
  - M4 makes `durable_profiles::prepare` fail on a managed daemon.

  T19 must therefore declare the composition its real-provider run uses (see "Notes for T19/T20").

**B2 notes.**
- `purge.rs:14-116` recognizes every artifact written into a runtime directory, using exact names plus four bounded parsers: no sign, no leading zeros, range-checked sequence numbers, a fixed 64-hex form, and no glob. The writers are the host supervisor, watchdog, retirement, crash, proof, quiescence and `check-runtime.json`, plus the runner's `runtime.sqlite` with its `-wal`/`-shm`/`-journal` files.
- Links, directories, `nlink≠1` and a foreign uid are refused before the first unlink (`purge.rs:131-147`).
- The lock is unlinked last, after a final `verify` (`purge.rs:175-187`).
- Composed real retire → purge and crash → reconcile → retire → purge tests (`authorization_session/purge_runtime.rs:261-310`, class b) and a CLI → daemon → host journey (`cli/tests/support/supervised_purge.rs`, class b) exist.

**M3 notes.**
- Launch preflight checks components (`launch.rs:418-452`).
- The manifest records `runner_component_sha256`.
- `ComponentCatalog` serves a retained Launch only its original components.
- Revision-switch retirement and recovery are tested (`packaged`, class b).

**M5 notes.**
- `RunnerEvent` and `RunnerOutput` are strict enums with `deny_unknown_fields` (`rpc/src/durable_execution.rs:193-243`), parsed typed in `recovery.rs:569-583`.
- The dead v1 request variants are removed.

**M6 notes.**
- Purge now has a CLI → daemon → host test.
- There is still no daemon-level test of a follow-up marked `unknown` and re-sent in a later session ending with exactly one admission. `recovery.rs:513-545` commits `Unknown`, then sends, and relies on runner deduplication. That deduplication is tested only by `runner/test/resume-runtime.test.mjs:101-128` (class a, faux provider). T21 defers this explicitly (`docs/research/pi-durable-t21/README.md:140`).

### New findings (second pass)

#### B3 — BLOCKER: any daemon restart permanently strands live durable Assignments (M1 escalated)

AC7, AC8, AC11, AC12, AC17 and AC22; Spec Implementation Decisions. Responsible deliverables: T09, T10 and T23. Rules: SAFE-2 (retained uncertainty must stay reconcilable) and TEST-2.

The first pass rated M1 MAJOR because no production launch existed. T23 makes it reachable. Verified by hand:
- **The registry does not survive a restart.** The `Launcher` registry holds only children the current daemon spawned. A restarted daemon starts empty and never adopts (`launch.rs:370-373`).
- **The old host fences itself and never exits.** It loses its stdin, so renewal stops (`runner-host/src/watchdog.rs:163-165`). At lease expiry the watchdog:
  - writes `watchdog.json`;
  - sends TERM, then KILL;
  - then *"Retain ownership indefinitely"* (`watchdog.rs:447-484`).

  The host process holds the flock forever.
- **Automatic recovery refuses.** `reconcile_crash` returns `false` if the exclusive lock cannot be taken, or if `watchdog.json` or `retirement-intent.json` exists (`server/src/durable_authorization/retirement.rs:117-126`). Recovery quiescence rejects the same markers (`runner-host/src/quiescence.rs:54-61`).
- **Explicit retirement fails.** `Controller::retire` (`retirement.rs:158-252`) has no `Owned` entry after a restart, so it sends no retirement. It then requires `ExclusiveInspection::acquire` and an exact retirement receipt (`:237-239`). The orphaned host holds the lock and was never sent a retirement, so this returns `uncertain` on every retry.
- `resolve_unknown_agent_resources` retires only `provision-intent` (`store/src/lib.rs:3142`).

**Consequence:**
- Every daemon restart while a durable Assignment is live leaves an orphaned host holding the flock. That includes a crash, a host reboot of the daemon only, and every Forgeup Revision switch or upgrade.
- The Assignment's capacity is reserved permanently.
- Neither recovery, retirement, purge nor any audited operator command can resolve it.
- `docs/durable-execution.md:318-325` tells operators to *"plan for … recovery or retirement after any daemon outage"*. Neither is possible.

The `packaged` scenario's "switch" only works because the runner crashed (host exited with a crash receipt) *before* the new composition. A real daemon-process restart with a live runner is not exercised (T23 README, "Not claimed").

**Required remediation:**
1. Add a watchdog-fenced reconciliation path. For example:
   - after the fence, the original host, which is the subreaper, proves descendant absence;
   - it writes a protected exact receipt and releases or permits inspection of the lock;
   - Forge accepts that receipt under exclusive inspection.

   Alternatively, add an explicit, audited operator retirement for unowned durable runtimes that verifies process absence by retained identity. Either way, capacity is released only on proof.
2. Add a real daemon-process restart test with a live runner: restart → expiry → reconcile → retire or recover.
3. Correct `docs/durable-execution.md:318-325`.

#### B4 — BLOCKER: a launch that fails after its claim can never be retired

AC11, AC12, AC13 ("explicit authorized intervention remains possible") and AC17. Responsible deliverable: T23. Rules: TX-1, SAFE-2, RUST-3 and TEST-2. Verified by hand.

There are two separate gaps.

**1. The claim and the reservation are separate transactions.**
- `begin_agent_provision` commits the `provision-intent` claim (`store/src/lib.rs:3029`). `reserve` (`launch.rs:562`) re-authorizes in a *new* transaction that requires `state='provisioning'` and the current generation (`store/src/durable_execution.rs:69,84`).
- A close, a generation bump or a daemon restart between them leaves a permanent claim with no runtime row.
- From then on:
  - `provision` returns `durable-launch-claimed`;
  - `observe` and `recover` return Transient;
  - `Adapter::retire` fails at `self.runtime()?` (`retirement.rs:313-323,413`), which requires the runtime row.

**2. Failures after reservation strand the Assignment and orphan the host.** This covers:
- a failed `check_runtime::write`;
- `begin_agent_launch` returning `false` (`launch.rs:602`);
- a spawn failure (`:606-615`);
- a host that never sends valid `owned` evidence (`:768-777`);
- a failed `bind_inspected` (`:793-797`).

In each case the driver retains a failure and returns. In the last two, the `Child` is dropped without being killed, reaped or registered with retirement, so the host process is orphaned and stays a zombie. Later, `Controller::retire` finds neither an `Owned` entry nor a revoked lease (`retirement.rs:189-190`) and returns `uncertain` forever.

**Consequence:** a transient fault during launch makes the Assignment permanently unretirable and its `durable-runtime` capacity permanently reserved. There is no operator path. `launch_tests.rs` has no test for write, spawn, `owned` or bind failure.

**Required remediation:**
- Make the claim and the reservation one atomic precondition (TX-1), or reconcile a claim with no runtime through `resolve_unknown_agent_resources`.
- On every post-spawn failure, register the original Child and pipe with retirement, or kill and reap it under the subreaper with a retained receipt.
- Give a reserved-but-unspawned runtime a retirement path that proves it was never spawned.
- Add fault tests for each listed failure.

#### M2′ — MAJOR: documentation now contradicts the composed recovery path

AC23. Responsible deliverables: T17 and T23. Rule: VERIFY-2.

T23 composes `recovery` together with the launcher (`launch.rs:1311-1323`), and app reconciliation calls `recover` automatically (`app/src/lib.rs:1858,1907`). Any daemon with schema 2 bundles or `FORGE_DURABLE_RUNNER_HOST`/`ENTRY` therefore has automatic recovery configured. These documents still say otherwise:

| Location | Problem |
| --- | --- |
| `docs/assignment-recovery.md:133-134, 149, 254, 325, 605` | Says recovery is unconfigured, disabled, or "remains disabled". |
| `crates/store/README.md:152, 257-258, 323` | Says no production daemon creates a claim. `claim_recovery` runs at `recovery.rs:162`. |
| ADR-0012 line 5 | Says recovery is disabled. |
| `docs/assignment-recovery.md:286` | Lists only env vars for transcripts, though managed daemons use `--durable-runner-bundle`. |
| `server/src/main.rs:101` | Calls `FORGE_DURABLE_OPENAI_KEY_FILE` "preflight" only. It is also the launch credential (`launch.rs:532`). |
| `runner/README.md:46, 84, 208, 400, 460` | Still says launch is "refused until T23" or "remains disabled". |
| `runner/README.md:169, 216, 446` | Still references `main.ts`. |
| `docs/development.md:100-110` | Omits `FORGE_DURABLE_RUNNER_HOST`/`ENTRY`. |
| "Production durable dispatch remains disabled" (in several docs) | Conflicts with the gate's own states, `Withheld` (default) and `Disabled` (rollback). |
| `docs/durable-execution.md:118-119` | Key-exposure claim still unqualified (first-pass M2). |

**Remediation:** correct each location, and use the gate's terms.

#### MINOR (second pass)

1. **Stop timeout and retirement.**
   - `STOP_TIMEOUT` (10 s) is shorter than `OWNED_TIMEOUT` (30 s) and `ADMISSION_TIMEOUT` (120 s) (`launch.rs:44-46`). Stops are polled only in `run_loop`, so retirement during bootstrap or admission times out and needs retries.
   - `stop_for_retirement` holds the `slots` mutex for the whole wait (`launch.rs:651-663`), which blocks `state()` and `provision` for every Assignment.
2. **RUST-3: renewal error discarded.** In `let _ = self.renew_if_due()` (`launch.rs:934`), a `boottime_millis` error (`:909`) stops renewal silently, with no fence and no diagnostic.
3. **SAFE-2 classification: oversize output looks like a crash.** A stdout line over 64 KiB ends the reader without an `Err` (`launch.rs:698-703`). A live host is then classified `Crashed` and dropped after `reap` (`:860`). The flock keeps this safe, but liveness is lost.
4. **RUST-1: stringly-typed classification.** `adapter_error` classifies by matching code strings (`launch.rs:1154-1165`), and `profile_record_type: "task".into()` is a bare string (`:572`).
5. **AC19: attestation gaps.**
   - The Launch component hash is computed by the `contract.ts` being attested and excludes `node_modules` (`runner/src/contract.ts:11`).
   - Between daemon start and launch, only stat fingerprints of the host and Node catch tampering.
   - Recovery under retained components (`recovery.rs:140`) does not preflight the retained host.
6. **Purge edge cases.**
   - Removal is path-based, not `unlinkat` on a directory fd. A same-user rename between `verify` and unlink is outside the stated threat model.
   - A file created mid-purge makes `remove_dir` fail after the lock is removed. Every later retry then refuses (`purge.rs:167-190`). This is fail-closed, but needs manual repair and is untested.
   - Recognized files are not checked for 0600 mode (`purge.rs:140`).
7. **Purge reporting.**
   - A crash between directory removal and `finish_purge` makes the retry journal `already-absent`, which contradicts "nothing was removed" (`app/src/durable_execution/purge.rs:82-83`). This understates, so it is conservative.
   - On an exact-retry replay, the human output still says "this operation removed".
8. **Test caveats.**
   - `supervised_recovery.rs:69-74` polls `recovery.recover()` directly after one `reconcile_agent_dispatch`. That poll can perform the recovery itself, so the reconciliation scan alone is not shown to recover.
   - Provision runs the three preflight subprocesses twice, through `validate_provision` and `provision`.

### Standards confirmed (second pass)

- The single-launch claim is atomic (`INSERT OR IGNORE` in an IMMEDIATE transaction).
- `durable_execution::admit` uses short separate transactions and holds none across the pipe (TX-4).
- `begin_send` marks the request Unknown before sending, then observes; there is no blind replay (SAFE-2).
- `check-runtime.json` holds paths only, no secret bytes, and is written `O_EXCL` into a freshly minted directory. It is input, not the writable store (SEC-1, AC6).
- The host is spawned with `env_clear` and explicit arguments (SEC-3).
- The only new production `expect` (`launch.rs:498`) guards a locally evident invariant (RUST-2).
- Purge patterns are bounded and canonical (SAFE-4).

### Acceptance-criteria trace: changes since the first pass

Rows not listed keep their first-pass assessment, except that the general **B1** caveat ("no production path") is replaced by "production path exists; withheld; no class (c)".

| AC | Change at `646abd9` |
| --- | --- |
| AC1 | Production adapter composed (b, tracer uses `launch::compose`). Withheld by default. M4 blocks managed daemons. |
| AC3 | Shipped `check-entry.ts` composes bootstrap, profile and model key before intent or store (b, 7 real-host refusal tests). Real-provider path never run (c missing). |
| AC4 | Production launch mints fresh runtime, conversation and store identities per Assignment (b). |
| AC5 | Production `PipeTransport` plus typed events (M5 resolved). Unknown launch admission is fenced, not retried (minor 3). |
| AC7 | Recovery composed in production. **B3**: unavailable after a daemon restart with a live runner. |
| AC8 | **B3**: expiry after daemon loss has no reconciliation. |
| AC11 / AC12 | **B3**, **B4**: capacity is retained (safe), but retirement cannot complete. |
| AC13 | **B4**: "explicit authorized intervention remains possible" fails for stranded launches. |
| AC15 | M6 follow-up part still open. |
| AC16 | **B2 resolved** (b). |
| AC17 | **B3**/**B4**: reconciliation and retirement can be impossible regardless of source availability. |
| AC19 | Component preflight, host packaging and the Launch-hash catalog are in place (b, `packaged`). `package-bundle.mjs` is unexercised by tests. Minor 5. |
| AC21 | Gate and rollback cover the production composition (b, `rollback` scenario). |
| AC22 | **Unrun:** real provider, daemon-process restart with a live runner, any managed-daemon launch. Blocked by B3, B4 and M4. |
| AC23 | **M2′**. |

### Notes for T19/T20 (not findings)

- No shipped command grants `Controlled` launch eligibility. This is deliberate: `docs/durable-execution.md:15-20` says new launches stay withheld in every production Revision. T19's real-provider run therefore has to use a test composition that calls `enable_controlled`. It must state that, and must not present a packaged-daemon run that never happened.
- M4 means only a manual daemon can supply a model credential. T20 must either resolve this or limit the handoff to manual daemons.

### Required next steps (second pass)

1. **Operator decision:** assign B3 and B4, plus M2′ and M6, to a responsible deliverable. Closest owners: T23 for B4; T09/T10 with T23 for B3. The plan forbids silently adding Tasks.
2. After the corrections, re-review the corrected snapshot and record it here as a third pass. T19 consumes only that snapshot.

**T18 status:** the second-pass review deliverable is complete and records two blockers. T18 stays unchecked.

---

## First pass — 2026-11-20, snapshot `1fc65a5`

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
