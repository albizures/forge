# Corrections for T18 second-pass M2′ and M6

These are corrections to the responsible deliverables named by the second-pass review.
They are not a new Task:
- **M2′:** T17 and T23.
- **M6:** T12 and T23.

They were made after T26 (`93b983c`), before the third-pass review. No policy or
behavior changed apart from one new test and one new test-fixture marker.

## M2′ — documentation contradicted the composed recovery path

Each listed location now uses the release gate's terms:
- new durable launches are **withheld** by default, or **disabled** after a rollback;
- automatic same-attempt recovery of existing durable Assignments is composed whenever
  matched runner components are configured;
- the gate does not apply to existing work.

| Location | Correction |
| --- | --- |
| `docs/assignment-recovery.md` | Interrupted-effects, recovery heading/anchor, production-composition paragraph, supervised-retirement and transcript-component text. Managed daemons are described with `--durable-runner-bundle`/`--durable-retained-runner-bundle`. |
| `docs/durable-execution.md` | Anchor link updated. The key-exposure claim is qualified: the key *path* is in the manual daemon's environment (inherited by hooks, `git`, `gh`); the host is started with a cleared environment. |
| `crates/store/README.md` | The T05 and revision-20 sentences are dated. The recovery-claim paragraph names the composed `claim_recovery` caller. |
| ADR-0012 | The recovery-disabled sentence is replaced. |
| `runner/README.md` | Increment-history statements and `main.ts` references are marked as past increments, naming `test/synthetic-runner.ts` and `src/check-entry.ts`. |
| `docs/development.md` | Adds `FORGE_DURABLE_RUNNER_HOST`/`ENTRY`. |
| `crates/server/src/main.rs` help | `FORGE_DURABLE_OPENAI_KEY_FILE` is also the launched runner's credential. |
| `docs/state-upgrades.md` | Target schema 25, with the 23 → 24 and 24 → 25 steps. It was already stale at 23. |

## M6 — daemon-level unknown follow-up re-send

New class (b) test:
`crates/runner-host/tests/authorization_session/recovery_reconciliation.rs::unknown_follow_up_is_resent_to_the_next_session_with_exactly_one_admission`.
It runs a real host, the Node runner and SQLite through the application's reconciliation
scan and the production recovery controller:

1. A recorded follow-up is delivered while the original answer is still in flight (new
   fixture marker `controlled-async-model`, a non-blocking delayed faux answer).
2. The runner commits it (`replayed: false`, admission `N`).
3. The fixture drops the acceptance event and SIGKILLs the worker (marker
   `controlled-drop-followup-once`). The daemon is left with delivery `unknown`.
4. The host records crash proof, and the recovery controller resumes the same attempt in
   a new session.
5. The daemon re-sends the `unknown` operation. The runner finds its committed request
   and replays (`replayed: true`, the same admission `N`), and Forge records `delivered`
   with admission `N`.
6. There is exactly one follow-up record and the attempt is unchanged.

**Finding (limitation, not changed):** recovery resumes only an unsettled original
submission (`inspectRecoveryCandidate`). If a follow-up was committed after the original
answer settled (for example during a missing-Outcome intervention) and the session then
crashes, recovery refuses with `runner-recovery-intervention-required`. The follow-up
stays `unknown`. This fails closed, and retirement remains available. The first attempt
of this test exposed it: the blocking `controlled-slow-model` answer settled before the
follow-up was read.

## Gates (after both corrections)

| Gate | Result |
| --- | --- |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 926 passed, 0 failed, 1 ignored (pre-existing opt-in) |
| `durable_supervised_tracer` | pass: 25 scenarios |
| `runner`: `npm run check` / `npm test` | pass / 99 of 99 |
