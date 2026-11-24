# Durable release verification

What a Forge Revision needs before durable launches can be enabled for it, and how to
re-establish the upstream pi-durable behavior the runner depends on. Each verified Revision
has its own record in this directory, for example [Revision `6a2196a`](6a2196a.md). Operating
durable Assignments is covered in [Durable Task execution](../durable-execution.md).

## Requirements

Before default durable dispatch can be enabled for a Revision, the release owner needs a
retained verification report that names **that exact Revision**, the runner protocol and
dependency versions, the commands run, expected versus observed results and any unrun gates. It
must include a real-provider Implementation → Review cycle with authenticated Outcomes and exact
evidence, and deterministic coverage of duplicate admission, daemon and runner crashes, lease
expiry, interrupted effects, revoked recovery, missing Outcomes, incompatible components and
incomplete retirement. Upgrades renew it. An unrun or failed gate is a blocker. Repository gates
and the real-provider procedure are in [development verification](../development.md#verification);
records live in this directory.

Readiness for a Revision ends, and review plus verification must be renewed on the new exact
Revision, after any of:

- a change to the implementation, runner sources, lock file, pinned Node or pi-durable, runner
  protocol, store schema or host. Any byte under `runner/src`, comments included, changes the
  runner component hash and ends readiness;
- a platform, model or authentication mechanism outside the supported matrix;
- an unexplained failing or intermittent release-gate scenario;
- an operator rollback (`durable-launch-disable`), which is sticky for the store.

A later change that touches only documentation or comments outside `runner/src` does not end
the reported Revision's readiness by itself. The report still names the exact Revision it
verified, and the next renewal covers such changes.

## Upstream pi-durable 1.5.2 constraints

The runner depends on pi-durable behaviour that its documentation does not guarantee. These
facts were established by probing the pinned 1.5.2 sources and shape the runner's design;
the probes live in `runner/test/upstream/`.

- **Hooks do not deny.** A throwing `beforeRequest` hook is caught and reported by the
  scheduler, and the model request is still sent. Authorization is therefore enforced by
  wrapping the actual provider and the actual tool `execute` (`guardProvider`, `guardTool`),
  never by hooks.
- **Compaction bypasses generation hooks.** Compaction calls `models.completeSimple`, which
  skips `beforeRequest`; only the provider-level guard covers it. Automatic compaction stays
  disabled until it has its own threshold and retry tests.
- **Submission IDs do not detect changed content.** A `requestId` lookup rejects a different
  submission *type* but returns the original submission for changed *content*. Forge persists
  a canonical payload fingerprint per operation ID and rejects conflicts before calling the
  runtime.
- **Writable open recovers.** `Harness.open` immediately reconciles interrupted tasks
  (`openTasks`), and resuming after an interrupted tool returns an "interrupted" result and
  lets the model continue. All ownership, authority and interrupted-effect checks therefore
  run *before* the writable open, not just before `resume`.
- **Tool checkpoints.** `pi.tool` v3 commits a `call` checkpoint (no effect yet) and then an
  `execute` checkpoint holding the arguments and replay policy *before* entering the tool;
  the result is committed separately. Upstream replays only when both the recorded and the
  current policy say safe. Forge treats an unsettled `execute` as an interrupted effect even
  if it is labelled replay-safe.
- **No physically read-only open.** The stock SQLite opener always runs migrations, and
  chord's initializer performs an unconditional `INSERT OR IGNORE`, so a read-only connection
  fails with SQLite error 8. `Harness` `readOnly: true` only blocks submit/resume; storage is
  still opened writable. Forge therefore ships a derived read adapter
  (`runner/prepare-reader.mjs`) that validates schema version 2 exactly, never migrates and
  opens SQLite read-only. Patching upstream or filtering SQL was rejected.
- **Durability.** The Node adapter uses WAL with `synchronous=NORMAL`: durable across process
  crashes, not power loss.
- **SDK retries.** With caller retry options set, the pinned OpenAI SDK still made exactly one
  request on a 429; Forge disables provider retries anyway (`maxRetries: 0`).

### Re-validating an upgrade

Changing pi-durable, pi-ai, chord or Node changes the runner component hash and invalidates
readiness. Before re-verifying the release, re-establish each constraint above with
`cd runner && npm run test:upstream`, and additionally check: the five provider request paths
(stream, simple, deferred fetch, image, classifier) all pass through the provider guard; the
derived reader's upstream base still hashes as `prepare-reader.mjs` expects; the runtime
schema version is still 2; a SIGKILL after an effect but before its result is still reported
as interrupted and not re-run; and Node still satisfies pi-durable's engine floor
(`>=22.19.0` for 1.5.2). `prepare-reader.mjs` refuses an upstream `storage.js` whose sha256
differs from the inspected 1.5.2 file, so the derived reader must be re-derived and reviewed
for any upgrade. A changed answer is a design question, not a test to update.

