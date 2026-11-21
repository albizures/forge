# T19 — Verify packaged integration, recovery and a real-provider Check cycle

**Result: deterministic verification passes. The real-provider release gate is UNRUN, so
default-rollout approval is WITHHELD.**

These are local verification checks, not release approval (Task T19, "Verification and
evidence").

## Snapshot and identities

| Item | Value |
| --- | --- |
| Reviewed implementation snapshot | `2097267` ([T18 third pass](../pi-durable-t18/README.md#third-pass--2026-11-21-final-snapshot-2097267)) |
| Verified tree | `6cd2e44`. It differs from `2097267` only in two documentation files (the T18 report and the task checklist): `git diff --stat 2097267 6cd2e44`. |
| Forge Revision reported by the built host | `6cd2e44f801f6fe5d84330569d5ab74810ba4172` (clean) |
| Runner protocol / durable runtime schema | 1 / 2 (`forge-runner-host --compatibility`) |
| Forge store schema revision | 25 |
| Runner component hash (`contract.ts::componentHash`, and the bundle manifest) | `8991977e0e4cb7c4468875f5cbec11ac69403477e14d5378c92a7346a4af28a0` |
| Pinned dependencies | `@earendil-works/pi-durable` 1.5.2, `@earendil-works/pi-ai` 1.5.2, Node 24.18.0 |
| `runner/package-lock.json` sha256 | `69dfc7381ba7c4e6903970afdb42714e048c6abc851748f136c021a305b3bdf1` |
| Bundle tree sha256 (from `package-bundle.mjs`) | `89f09c62e5df092f47a36539c8d27694d708a62de4b82ea3337d96c6a1541760` |
| Toolchain | rustc/cargo 1.98.0. Node 24.18.0 was first on `PATH`: a real binary, because the Volta shim breaks `durable_check_bootstrap`. |
| Host | Linux 7.0.0-43-generic, x86_64. Single trusted local user. Bundled SQLite (rusqlite 0.38, `bundled`). |

## Gates

All gates were run from the Forge root.

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --check` | pass |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| Tests (normal parallel mode) | `cargo test --workspace --no-fail-fast` | pass, twice in a row: **931 passed, 0 failed, 1 ignored** (the pre-existing opt-in Herdr Pi probe). Summary: [workspace-test-summary.txt](workspace-test-summary.txt). |
| Supervised tracer (inside the run above) | custom harness `durable_supervised_tracer` | pass: **25 scenarios** |
| Whitespace | `git diff --check` | pass |
| Runner typecheck / tests | `cd runner && npm run check && npm test` | pass / **99 of 99** |
| Packaging | `node runner/package-bundle.mjs --out <scratch> --forge-commit <HEAD> --runner-host target/debug/forge-runner-host` | pass. The schema 2 manifest records the same component hash as the source attestation. The host refuses a mismatched commit or protocol. Not registered with Forgeup (no operator state changed). Registration and verification with Forgeup are covered by the `packaged` scenario. |
| Shipped skills | — | not affected by T21–T26 or the corrections |

**Earlier intermittent failure.** During T26/T18 work, one full run failed
`committed_model_authority_expires_during_renewal_outage…`, a real 30-second leased-expiry
test. Its suite passed in isolation three times, and both final full runs passed. It is
**not claimed fixed**. See the [T26 evidence](../pi-durable-t26/README.md#gates).

## Deterministic cross-process fault scenarios

The scenarios run real processes: the real `forge-runner-host`, the Node runner, SQLite and
Git, with a faux model provider. "Tracer" means
`crates/cli/tests/durable_supervised_tracer.rs`, which runs the production composition
(`launch::compose`). Its test-only parts are listed in
[T18](../pi-durable-t18/README.md#coverage-classes-second-pass).

| Required scenario | Evidence (all pass) |
| --- | --- |
| Duplicate admission | Tracer duplicate and fresh-composition provision (never respawns a claim); `launch-fault-*` re-dispatch; store `claim_and_reservation_commit_together_or_not_at_all`; runner `process.test.mjs` idempotent admission |
| Competing writer | `runner-host/tests/authority.rs` (flock, lookalike/unrelated child, exited supervisor); `sealed_runtime_directory_refuses_every_later_host` |
| Daemon/runner restart ordering | Tracer `daemon-restart`, `-retire`, `-tool`, `-exit` (a separate daemon process SIGKILLed); `launch-fault-restart-{runtime-input,spawn,stall,grant}`; `recovered`; `committed_but_undelivered_{renewal,first_grant}` (pipe loss) |
| Lease expiry | `committed_*_authority_*_outage…` (real 30 s); `lease_expiry_without_restart_…`; tracer daemon-restart (fence → receipt) |
| Stale/revoked recovery | `recovery_controller_refuses_missing_foreign_and_revoked_proof_before_spawning_or_claiming`; tracer `revoked`; `predecessor_tests` negative cases |
| Interrupted external effect | Tracer `daemon-restart-tool` (effect blocks recovery, then resolution, then retirement); T08 real Bash/publication fault tests |
| Missing Outcome | Tracer `missing` and `rejected`; `recovery_reconciliation` missing-outcome intervention |
| Source loss | T09 retirement with lost profile/auth sources (retirement suite) |
| Incomplete retirement | `refused_cancellation_…`, `escaped_descendant_…`, `never_granted_…` (`durable-launch-ungranted-unproven`), tracer `daemon-restart-retire` (tampered receipt → `durable-runtime-fence-unproven`) |
| Profile migration retries | `check_profile_migration` store and CLI tests (9 migration tests in the run) |
| Deletion safety | Purge suites (19 purge tests), including CLI → daemon → host, failed-launch purge and refusal of unknown or near-miss files |
| Packaging mismatch | Tracer `packaged`: a Revision switch without the original bundle gives explicit refusals, and with it the original host/Node recovers. Launch preflight gives `durable-components-incompatible`. The packaging run above. |
| Rollback | Tracer `rollback`: sticky disable; status and retirement of live and closed-uncertain work still work |
| Herdr / Grilling retained behavior | 42 Herdr/Grilling tests in the run, plus release-gate tests (no fallback, Grilling on Herdr) |
| Operator CLI/RPC controls | Tracer journeys drive `assignment-status`, `assignment-transcript`, `assignment-followup`, `assignment-retire`, `assignment-purge-runtime`, `resume`, `durable-launch-status`/`-disable` through the real CLI → daemon |

**Observed safety properties.** No scenario showed any of the following:
- duplicated external effect;
- execution under a revoked or undelivered lease;
- capacity released without host or store proof;
- inferred completion.

These are assertions in the tests above, not inspection by eye.

## Acceptance-criteria mapping

The per-criterion trace, with implementation locations, test classes and gaps, is the
[T18 third-pass AC table](../pi-durable-t18/README.md#acceptance-criteria-trace-at-2097267).
This run confirms that every listed class (a) and (b) test passes on the verified tree.

**AC22, the release verification gate, is NOT met:**

| AC22 element | Status |
| --- | --- |
| Deterministic duplicate admission, daemon and runner crashes, lease expiry, interrupted effects, revoked recovery, missing Outcomes, incompatible components, incomplete retirement | **Run, pass** (above) |
| Real-provider Implementation → independent Review cycle: authenticated bootstrap/Outcomes, changed output commit, exact Review binding, retirement before successor | **UNRUN**: no operator-authorized, isolated real-provider credential and environment was provided. Per the Task, live work and credentials must never be used without authorization. Only the faux-provider `continuation-chain` scenario covers this shape (class b). |
| Managed (Forgeup) daemon launch | **UNRUN / not possible**: M4, a managed daemon has no model credential |
| Review handback/Continuation and publication/unchanged rules | Covered by the retained integration gates in class (b) (`continuation-chain`, rollout tests). Publication with a durable pin was not re-exercised (T16 note). |

## How to run the missing real-provider gate

Run this only with operator authorization, and never against ordinary live work.

1. **Environment.** Use a dedicated Workspace and store, a scratch Git repository, and a
   **manual** daemon. A managed daemon cannot work because of M4. Configure:
   - `FORGE_DURABLE_RUNNER_HOST` and `FORGE_DURABLE_RUNNER_ENTRY`, pointing at a bundle
     built from the verified Revision;
   - `FORGE_DURABLE_NODE` and `FORGE_DURABLE_EFFECT_INSPECTOR`;
   - `FORGE_DURABLE_OPENAI_KEY_FILE`, pointing at an isolated `0600` key file.
2. **Launch eligibility.** Every shipped Revision withholds new launches, and no command
   enables them. The run must therefore use a test composition that calls
   `LaunchControl::enable_controlled`, and must say so.
3. **Cycle.** Register a durable Task profile, pin Implementation and Review, then
   dispatch. Verify:
   - authenticated bootstrap;
   - an accepted Implementation Outcome with a changed commit;
   - safe retirement;
   - a fresh Review Assignment bound to that exact commit;
   - an accepted Review Outcome;
   - no next Check before acceptance and retirement.
4. **Evidence.** Record commands, expected and observed results, and identities.
   Redact every credential.

Any implementation change after `2097267` invalidates these results and needs renewed
review and verification.

## Remaining limitations

- No class (c) evidence exists. M4 limits supported operation to manual daemons.
- The test gaps and open minors listed in the
  [T18 third pass](../pi-durable-t18/README.md#still-open-non-blocking) still apply.
- Process-crash tests do not establish durability against physical power loss.
- The trusted-local-host boundary applies. Same-user tools can read anything the user
  can, including the model key file.
