# T17 — Domain records and operator documentation

Documentation deliverable on top of T16 (`e94fb4f`). Production durable dispatch and automatic
recovery remain disabled. No live profile was migrated, no rollout changed and no shipped
skill was edited.

## Delivered

- **Operator guide** [`docs/durable-execution.md`](../../durable-execution.md): release state
  (launches withheld, no enable command), execution model, the distinction between
  readiness, authorization, liveness and verified absence, a supported
  platform/model/tool/context/authentication matrix, packaged-runner installation, profile
  preparation, explicit pin migration, rollout/rollback, status/transcripts, follow-ups,
  missing Outcome, lease timing and outage behavior, interrupted effects, retirement,
  retention/purge, prohibited shortcuts, trusted-local-host limits and release verification
  requirements.
- **Glossary** (`crates/CONTEXT.md`, the existing runtime context; no new root glossary):
  Forge Workspace and Forge Dispatch Workspace changed as proposed. Agent Assignment now says
  it binds the Check's *effective* revision at reservation, which reconciles it with T13's
  migration. Assignment Authorization Lease (T05) and Assignment Execution Recovery (T10)
  match the proposals. Check (T13) matches; Agent Profile (T13) contains the proposal plus
  pre-existing Skill/freezing clauses, so it was kept. Task, Continuation and Assignment
  Session are unchanged. pi-durable's internal tasks are called "execution tasks" in the guide
  and in ADR-0014, with no glossary entry.
- **ADRs:** proposed ADR-0012 (headless execution) is applied as
  `0014-headless-durable-task-execution.md`, because 0012 and 0013 had already been used for
  the proposed recovery (0013) and migration (0014) decisions. Each file records that
  renumbering. ADR-0005 and ADR-0008 now point to 0014 for their partially superseded Herdr
  provisioning/readiness requirements, and ADR-0007 notes that durable Assignments reuse it.
- **Updated operational docs:** `dispatch.md` (backend selection per Check; the Herdr
  configuration and launch sections are labelled Herdr-only; durable Checks need no Herdr
  socket, binding or Workspace readiness), `workspaces.md` (ownership table and runtime
  storage location), `assignment-recovery.md` (entry pointer; follow-up delivery is no longer
  described as unimplemented; the migration release-gate refusal; rollback links; stale
  "later Task" statements replaced), `agent-profile-policy.md` (heading, managed-daemon
  credential gap, Workspace-selection side effect), `development.md`, `runner/README.md`
  (current-state introduction instead of "synthetic-only"), `crates/docs/forgeup.md` (matched
  runner section), `crates/store/README.md` (schema 23 follow-up tables) and the README
  documentation index.
- **Tests:** the guide is added to `operator_guide_commands_parse_without_execution`, so every
  `forge` example in it is parsed by the real clap parser. The new
  `durable_guide_shows_every_durable_operator_command` test requires every durable operator
  command and the key limit statements to appear in the guide.

## Documentation-to-command checklist (observed)

Isolated fixture: a manual `forge-server` built from `e94fb4f` with its own `HOME`,
`FORGE_STORE_PATH` and port. The fixture also had a throwaway Workspace/Git Project, a synthetic
`0600` key file in a `0700` directory and two profiles (durable revision 2 and Herdr revision 3).
There were no real credentials, Herdr or network provider.

| Documented behavior | Command | Observed |
| --- | --- | --- |
| Durable profile preview/registration with explicit key file | `agent-profile preview/register … --record-type task` | Accepted; revision 2 `runtime: pi-durable`, `openai/gpt-4.1`, `off`, four tools |
| Launches withheld by default | `dispatcher durable-launch-status [--json]` | `withheld`, `new_launches_admitted: false`, three notes |
| Durable-pinned dispatch refused, no fallback | `dispatcher dispatch 2` (Check pinned to rev 2) | `invalid-check-profile`; Task stays Open, no Assignment |
| Migration to a durable revision refused while withheld | `record check-profile-migrate 1 … --check 1=2 --check 2=2` | `invalid-check-profile`; `check-profile-migrations 1` reports none recorded |
| Status/transcript/follow-up/purge scope check | each against an unknown Assignment | `durable-scope-denied`, nothing created |
| Transcript limit 1–100 | `--limit 101` | Rejected by the parser before any request |
| Rollback validation | `durable-launch-disable --operation-id 'bad id'` | Rejected before I/O with the allowed-character message |
| Sticky, idempotent rollback | `durable-launch-disable rollback-1`, then `rollback-2 --json` | First recorded; second returned `rollback-1` with `replayed: true` |
| Rollback survives restart | Restart daemon, `durable-launch-status --json` | `disabled`, original operation/timestamp |
| Dispatch after rollback | `dispatcher dispatch 2` | `invalid-check-profile` |
| Missing credential fails closed | Registration with no `FORGE_DURABLE_OPENAI_KEY_FILE` | `durable-model-auth-unavailable`, no path disclosed |
| Permissive key file rejected | Key file mode `0644`, preview | `durable-model-auth-unavailable` |
| Relative/partial component paths rejected | `FORGE_DURABLE_NODE=relative/node` | Daemon refused to start |
| No secret leakage | `grep -a` for the fixture key in store, lock and all daemon logs | 0 matches |
| `dispatcher status` is Herdr-oriented | `dispatcher status` | Herdr binding/socket blockers only; no durable readiness claim |

Not executed here: status, transcript, follow-up delivery, effect resolution, retirement and
purge against a **real** durable Assignment, and a Forgeup packaged install. These need a running
runner with a granted lease, which production composition refuses. They are covered by the
existing controlled tests the guide cites (`durable_check_bootstrap`, `durable_supervised_tracer`
(10 scenarios), runner-host `transcript_status`/`purge_runtime` and T15's packaged run), which
run in the workspace gate below. Herdr Grilling behavior after rollback was not re-run (no Herdr
here); T16's production-composition test covers it.

## Findings for later Tasks

- **Managed daemons have no model-credential mechanism.** Forgeup passes matched components but
  no key-file setting, and a managed daemon ignores `FORGE_DURABLE_*` environment values. So
  durable profiles cannot be registered and launches could not authenticate under a
  Forgeup-managed daemon. The guide documents this as a known gap. T19/T20 must resolve it or
  explicitly accept manual-daemon-only support.
- Registering a durable profile selects it as the Workspace Task profile, which makes Herdr
  readiness `task-profile-invalid`. This is documented with the reselect step; it is unchanged
  behavior.
- Agent Profile preview output for durable profiles still prints the Herdr-oriented "Pi flags"
  limitation lines. This is a cosmetic inaccuracy in existing CLI output, not changed here.

## Gates

Run from the repository root with real Node 24.18.0 first on `PATH` (the Volta shim breaks
`durable_check_bootstrap`). `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and `git diff --check` are clean.
`cargo test --workspace --no-fail-fast` gave 881 passed, 0 failed and 1 pre-existing opt-in
probe ignored, including both documentation-parser tests and all 10 supervised tracer
scenarios. Runner sources were not changed, so runner typecheck/Node tests were not rerun.
