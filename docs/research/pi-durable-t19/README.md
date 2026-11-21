# T19 — Verify packaged integration, recovery and a real-provider Check cycle

**Result: verification passes on Forge Revision `6a2196a`.** The deterministic gates pass, and the
real-provider Implementation → independent Review gate (Spec AC22) passed in 2 of 3 runs. The
third run failed on a network timeout before the model answered, which is explained below and was
handled safely. The gate also found a real defect, which was fixed and independently reviewed
before the final runs.

These are local verification checks, not release approval (Task T19, "Verification and
evidence"). The readiness decision is [T20](../pi-durable-t20/README.md).

## Snapshot and identities

| Item | Value |
| --- | --- |
| Verified Revision | `6a2196a2de17ebfc77eeac43d8124f12c0ea2155` (clean; reported by the built `forge-runner-host --compatibility`) |
| Implementation changes since the T18 third pass | M4 (`7f95ba3`, reviewed in the [M4 addendum](../pi-durable-t18/README.md#m4-addendum--2026-11-21-snapshot-206f999-plus-the-follow-up-fix)); the effect-free tool-failure fix `9a7bb77` + `e968686` ([review](../pi-durable-t18/README.md#effect-free-failure-addendum--2026-11-21-9a7bb77-and-e968686)); test-only commits `bacf2ef`, `02e5950`, `9ac67f6`, `6a2196a` |
| Runner protocol / durable runtime schema | 1 / 2 |
| Forge store schema revision | 25 |
| Runner component hash (`contract.ts::componentHash`, and the bundle manifest) | `5d32b0862d3082f5712d49fb36a55ddc9871d880ea7ba780c065efc1d9b1f70c` |
| Pinned dependencies | `@earendil-works/pi-durable` 1.5.2, `@earendil-works/pi-ai` 1.5.2, Node 24.18.0 |
| `runner/package-lock.json` sha256 | `69dfc7381ba7c4e6903970afdb42714e048c6abc851748f136c021a305b3bdf1` (unchanged) |
| Bundle tree sha256 (from `package-bundle.mjs`) | `9cad5c0fae170b860a24a8fa21dcc57c85f37e110f30c098c7d974216d3ed803` |
| Toolchain | rustc/cargo 1.98.0; real Node 24.18.0 first on `PATH` (the Volta shim breaks `durable_check_bootstrap`) |
| Host | Linux 7.0.0-43-generic, x86_64. Single trusted local user. Bundled SQLite. |

## Deterministic gates (on `6a2196a`)

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --check` | pass |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| Tests | `cargo test --workspace --no-fail-fast` | pass: **932 passed, 0 failed, 1 ignored** (the pre-existing opt-in Herdr Pi probe). The new `durable_real_provider` test is a no-op here (opt-in). |
| Supervised tracer | `durable_supervised_tracer` (inside the run above) | pass: **25 scenarios** |
| Whitespace | `git diff --check` | pass |
| Runner typecheck / tests | `cd runner && npm run check && npm test` | pass / **109 of 109** |
| Packaging | `node runner/package-bundle.mjs --out <scratch> --forge-commit 6a2196a… --runner-host target/debug/forge-runner-host` | pass. The schema 2 manifest records component hash `5d32b086…`, equal to the source attestation. Not registered with Forgeup. |

**Earlier intermittent failure.** `committed_model_authority_expires_during_renewal_outage…` failed
once during T26/T18 work under full load. It passed in every run since, including this one. It is
**not claimed fixed** ([T26 evidence](../pi-durable-t26/README.md#gates)).

## Deterministic cross-process fault scenarios

Unchanged from the previous T19 pass and re-run green above. Real processes, faux model provider.

| Required scenario | Evidence (all pass) |
| --- | --- |
| Duplicate admission | Tracer duplicate and fresh-composition provision; `launch-fault-*` re-dispatch; store `claim_and_reservation_commit_together_or_not_at_all`; runner `process.test.mjs` |
| Competing writer | `runner-host/tests/authority.rs`; `sealed_runtime_directory_refuses_every_later_host` |
| Daemon/runner restart ordering | Tracer `daemon-restart`, `-retire`, `-tool`, `-exit`; `launch-fault-restart-*`; `recovered`; `committed_but_undelivered_{renewal,first_grant}` |
| Lease expiry | `committed_*_authority_*_outage…` (real 30 s); `lease_expiry_without_restart_…`; tracer daemon-restart |
| Stale/revoked recovery | `recovery_controller_refuses_missing_foreign_and_revoked_proof…`; tracer `revoked`; `predecessor_tests` |
| Interrupted external effect | Tracer `daemon-restart-tool`; T08 real Bash/publication fault tests; **new:** `runner/test/effect-free-failures.test.mjs` (effect-free vs uncertain tool failures) |
| Missing Outcome | Tracer `missing` and `rejected`; `recovery_reconciliation`; **and real-provider runs below** |
| Source loss | T09 retirement with lost profile/auth sources |
| Incomplete retirement | `refused_cancellation_…`, `escaped_descendant_…`, `never_granted_…`, tracer `daemon-restart-retire` |
| Profile migration retries | `check_profile_migration` store and CLI tests |
| Deletion safety | Purge suites, including CLI → daemon → host |
| Packaging mismatch | Tracer `packaged`; launch preflight `durable-components-incompatible`; packaging run above |
| Rollback | Tracer `rollback` |
| Herdr / Grilling retained behavior | Herdr/Grilling tests and release-gate tests |
| Operator CLI/RPC controls | Tracer journeys through the real CLI → daemon |

## Real-provider gate (Spec AC22)

**Authorization.** The operator authorized one isolated real-provider cycle on 2026-11-21, using an
owner-only key file passed by path (never read or printed by the agent), billed `openai/gpt-4.1`
calls, and scratch Workspaces under `/tmp`. Retries and reruns of the same gate were covered by
that authorization. The final runs were executed in a second operator session at the operator's
direction.

**Harness.** `crates/cli/tests/durable_real_provider.rs` (opt-in: it runs only when
`FORGE_REAL_PROVIDER_KEY_FILE` is set and is otherwise a no-op). It composes:

- the production durable composition (`launch::compose`) behind the production release gate;
- the real `forge-runner-host` binary built from the same Revision, refused if stale;
- the **shipped runner entry** `runner/src/check-entry.ts` with the real OpenAI provider (no faux
  model);
- the real Forge CLI and daemon RPC, including transcript reading through matched inspectors;
- a scratch Workspace, store and Git repository; separately pinned Implementation and Review
  profiles (the reviewer gets only `read` and `bash`).

The **only test-only substitution** is launch eligibility, `LaunchControl::enable_controlled`:
every shipped Revision withholds new durable launches. No Herdr fallback exists (any call panics).

**Asserted per run:** matched Revision and component hash on the Launch; authenticated bootstrap
(context read); an accepted Implementation Outcome with a changed output commit that adds exactly
`hello.txt`; no Review before the Implementation is retired; retirement releases capacity from
proof; a fresh Review Assignment (new runtime and conversation) bound to exactly the output
commit, on its own pinned profile; an accepted Review Outcome with the same evidence; the Review
transcript contains no `git add`, `git commit`, `write` or `edit` call; the worktree HEAD is still
the output commit and clean; no API-key-shaped token in runtime storage or the repository.

### Run history

| Revision | Result | What it showed |
| --- | --- | --- |
| `bacf2ef` | fail | OpenAI refused the first call: the account had no credits. Forge recorded Missing Outcome, kept capacity, started no Review. |
| `02e5950` | pass (weak) | Full cycle in 36 s, but the single shared profile let the reviewer act as an implementer (it rewrote `hello.txt` with identical content and its commit failed with "nothing to commit"). Forge's own check would have refused a changed Review commit (Outcome evidence is re-observed and must equal the bound commit, on a clean worktree). The harness was strengthened: separate profiles and independence assertions. |
| `9ac67f6` | **fail: defect found** | After committing, the model read `.git/HEAD` (a file-not-directory in a worktree). The tool threw, `guardEffectTool` treated every throw as an interrupted effect and closed authority, and the Assignment ended Missing Outcome. Upstream tools throw for routine mistakes (missing path, `edit` text not found), so real models would hit this constantly. **Fixed** in `9a7bb77` + `e968686`: see below. |
| `9a7bb77` ×3 | Implementation pass ×2; Review fail ×2; 1 void | Both Implementations passed. Both reviewers reviewed correctly but did not submit: one answered in prose, the other (with no `write` tool) never created the Outcome file and falsely claimed success. Forge recorded Missing Outcome both times. Profile prompt issue in the harness, fixed in `6a2196a` (one exact heredoc submit command). The third run was void: the tree was dirty mid-run and the harness refused it. |
| **`6a2196a` ×3** | **pass, fail (network), pass** | See below. |

### Final runs on `6a2196a`

Run by the second operator session on a clean tree, with the host rebuilt and reporting
`6a2196a2de17ebfc77eeac43d8124f12c0ea2155`. Evidence was re-checked from the saved
`evidence-summary.json` files.

| Run | Result | Elapsed | Output commit (Implementation) | Review bound commit | Verdicts |
| --- | --- | --- | --- | --- | --- |
| 1 | **PASSED** | 25 s | `b415d4e` | `b415d4e` | Pass / Pass |
| 2 | failed | — | — | — | — |
| 3 | **PASSED** | 27 s | `2747e17` | `2747e17` | Pass / Pass |

**Run 2.** The first OpenAI request failed at connection level after 0.5 s:
`Connection error.: AggregateError (ETIMEDOUT)`. No tokens, no tool call. The runner disables
provider retries by design (`maxRetries: 0`), so the Implementation went Blocked with Missing
Outcome, capacity stayed reserved and no Review was dispatched. That is the specified safe
behavior. It is an infrastructure failure, not a model or Forge failure, and it is recorded as a
known limitation (no automatic retry of a transient provider error).

No API-key-shaped string appears in any saved transcript.

### The defect the gate found, and its fix

`runner/src/interrupted-effects.ts::guardEffectTool` now receives an `EffectMeter`. The coding tools
reach the filesystem and shell only through `meteredEnv` (the pinned pi-durable tools import no
`fs` or `child_process`), which counts every started `ExecutionEnv` operation except a fixed
read-only list, and counts content reads unless the target is a regular file or absent. A tool
that throws before any counted operation started, with neither lease nor call aborted, returns an
ordinary `isError` result. The Harness commits it as a completed call, so the model can recover and
recovery is not blocked. Everything else stays a fatal interrupted effect. Tests:
`runner/test/effect-free-failures.test.mjs` and the production-wiring case in
`runner/test/profiles.test.mjs` (mutation-checked).

## AC22 status

| AC22 element | Status |
| --- | --- |
| Deterministic duplicate admission, crashes, lease expiry, interrupted effects, revoked recovery, missing Outcomes, incompatible components, incomplete retirement | **Run, pass** |
| Real-provider Implementation → independent Review cycle: authenticated bootstrap/Outcomes, changed output commit, exact Review binding, retirement before successor | **Run, pass** (2 of 3 on `6a2196a`; the failure is an explained network timeout handled safely) |
| Managed (Forgeup) daemon launch | Configuration verified in-process (`packaged` scenario composes Forgeup's managed arguments including the key). A real `forgeup daemon start` with a real provider was not run. |
| Review handback/Continuation and publication/unchanged rules | Class (b) only (`continuation-chain`, rollout tests). |

## Remaining limitations and open items

- **Open minors from the independent review of the fix** (none blocking; details in the
  [addendum](../pi-durable-t18/README.md#effect-free-failure-addendum--2026-11-21-9a7bb77-and-e968686)):
  a stat-then-open race that a background process the agent started earlier could exploit to
  consume FIFO data on an `edit`; the stat check assumes absolute paths (all pinned tools pass
  them); a synchronous `statSync` can stall the runner on a hung network filesystem; the
  "concurrent" test is not concurrent; two nits.
- **No provider retry.** A transient provider/network error leaves the Assignment Missing Outcome
  until an operator follow-up or retirement.
- **Outcome submission relies on the profile's behavior text.** The real runs show gpt-4.1 can end
  with prose or claim a submission it did not make; Forge never infers completion, but an
  operator-written profile must give an exact submission command.
- The earlier T18 open minors and test gaps still apply
  ([list](../pi-durable-t18/README.md#still-open-non-blocking)).
- Process-crash tests do not establish durability against physical power loss.
- The trusted-local-host boundary applies. Same-user tools can read anything the user can,
  including the model key file.

Any implementation change after `6a2196a` invalidates these results and needs renewed review and
verification, including the real-provider gate.
