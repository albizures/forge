# T20 — Durable Task execution: readiness decision and release handoff

## Readiness decision

**GRANTED for Forge Revision `6a2196a` only** (runner component `5d32b086…`). Every release gate
passed on this Revision, and on 2026-11-21 the operator explicitly accepted every open
non-blocking review item as a documented limitation (see
[Accepted open items](#accepted-open-items-operator-decision-2026-11-21)).

Readiness is a release decision, not enablement. Today no shipped command admits new durable
launches; launch eligibility must still be implemented and reviewed as its own change (step 4).
Readiness does not carry over to any other Revision.

| Basis | Evidence |
| --- | --- |
| Review | T18 third pass on `2097267` (no blocker), plus two addenda for every later implementation change: [M4](../pi-durable-t18/README.md#m4-addendum--2026-11-21-snapshot-206f999-plus-the-follow-up-fix) and the [effect-free tool-failure fix](../pi-durable-t18/README.md#effect-free-failure-addendum--2026-11-21-9a7bb77-and-e968686). No open blocker or major. |
| Verified Revision | `6a2196a2de17ebfc77eeac43d8124f12c0ea2155` ([T19](../pi-durable-t19/README.md)) |
| Deterministic gates | **Pass:** fmt, clippy, 932 Rust tests, 25 real-process tracer scenarios, 109 Node tests, packaging |
| Release gate (Spec AC22) | **Pass:** real `openai/gpt-4.1` Implementation → independent Review cycle passed in 2 of 3 runs on `6a2196a`; the third failed on a connection timeout before the model answered and was handled safely (Missing Outcome, capacity kept, no Review). Earlier runs found and drove the fix of a real defect. |
| M4 (managed-daemon credential) | **Resolved:** `forgeup daemon start --durable-openai-key-file`. |

A failed or unrun release gate is a withheld result. It is not a conditional success.

The source behaves accordingly:
- `forge dispatcher durable-launch-status` reports `withheld` and
  `new_launches_admitted: false`;
- no shipped command can change that.

This handoff performs no rollout, no profile registration or selection, no pin migration,
no dispatch and no configuration change.

## What passed, and with which components

These identities are from the T19 run on `6a2196a`:

| Component | Identity |
| --- | --- |
| Runner bundle | schema 2; runner protocol 1; durable runtime schema 2 |
| Runner component hash (carried by every Launch) | `5d32b0862d3082f5712d49fb36a55ddc9871d880ea7ba780c065efc1d9b1f70c` |
| Bundle tree sha256 | `9cad5c0fae170b860a24a8fa21dcc57c85f37e110f30c098c7d974216d3ed803` |
| Dependencies | `@earendil-works/pi-durable` 1.5.2, `@earendil-works/pi-ai` 1.5.2; lock sha256 `69dfc738…b3bdf1` |
| Node | 24.18.0, bundled |
| Forge store | schema revision 25. Upgrade from 7–24 is explicit and offline: [state upgrades](../../state-upgrades.md) |
| Platform | Linux x86-64; single trusted local user |

**Compatibility matrix.** The supported and unsupported configuration is defined in the
operator guide:
[supported configuration](../../durable-execution.md#supported-configuration).

Supported:
- the Task Record Type only;
- model `openai/gpt-4.1` with thinking `off`;
- the tools `read`, `write`, `edit`, `bash`;
- a raw OpenAI key file, on a manual daemon or a managed one.

Everything else is rejected, fail-closed:
- Grilling;
- any other model or provider, and OAuth;
- Pi settings;
- a managed key file without a schema 2 runner bundle.

## What must happen before readiness can be granted

1. **M4 is closed.** Managed daemons take `--durable-openai-key-file`. **Done.**
2. **Run the real-provider gate** on the exact Revision to be released. **Done** on `6a2196a`
   ([T19](../pi-durable-t19/README.md#real-provider-gate-spec-ac22)). To repeat it on another
   Revision: `FORGE_REAL_PROVIDER_KEY_FILE=<owner-only key file> cargo test -p forge-cli --test durable_real_provider`
   on a clean tree with the host rebuilt, under operator authorization (billed calls).
3. **Decide the open review items.** Fix them or accept them explicitly: the
   [third-pass list](../pi-durable-t18/README.md#still-open-non-blocking) and the
   [effect-free fix addendum list](../pi-durable-t18/README.md#still-open-after-the-addendum-non-blocking).
   **Done: the operator accepted all of them on 2026-11-21**
   ([record](#accepted-open-items-operator-decision-2026-11-21)). Any later fix changes the
   implementation and requires renewed review and verification, including the real-provider
   gate.
4. **Implement eligibility.** Only after steps 1–3 pass on that Revision can launch
   eligibility be implemented and reviewed as its own change. Today no shipped command
   grants it, and it must not be added as a side effect of this handoff.

## Accepted open items (operator decision, 2026-11-21)

The operator accepted every open non-blocking review item as a known limitation. The permanent
list is in the operator guide:
[accepted limitations](../../durable-execution.md#accepted-limitations-release-of-revision-6a2196a).

## Conditions that revoke readiness, or require renewed verification

- **Any change** to the implementation, the runner sources, the lock file, the pinned Node
  or pi-durable, the runner protocol, the store schema or the host. The runner component
  hash or Revision then changes, and T18-style review plus T19 verification must be
  renewed on that exact Revision.
- **A different platform, model or authentication mechanism** from the supported matrix.
- **Any failing or intermittent** release-gate scenario that has not been explained.
  `committed_model_authority_expires_during_renewal_outage…` failed once under full load
  and is recorded but not claimed fixed. The one failed real-provider run on `6a2196a` is
  explained (connection `ETIMEDOUT` before any model output).
- **An operator rollback** (`durable-launch-disable`). It is sticky and immutable for the
  store.

## Operator checklist: deliberate adoption, once readiness is granted

**Do not perform it until launch eligibility exists (step 4)**, and only on `6a2196a`'s matched
components.
Every command below exists and is documented in the
[operator guide](../../durable-execution.md).

1. **Install matched components** for the Revision. Use
   `node runner/package-bundle.mjs … --runner-host …`, then
   `forgeup register … --runner-bundle …`, `forgeup runner verify <selector>` and
   `forgeup daemon start` ([install](../../durable-execution.md#install-and-run-the-matched-runner)).
   Keep every older Revision's bundle registered while it still has retained durable
   Assignments, so they run only on their own components (`forgeup runner resolve
   --component-hash`).
2. **Supply the model credential.** Use `forgeup daemon start --durable-openai-key-file PATH`
   for a managed daemon, or `FORGE_DURABLE_OPENAI_KEY_FILE` for a manual one. Either way it
   is an owner-only `0600` key file, kept outside every Managed Worktree. Stop a running
   managed daemon before starting it with the option.
3. **Prepare a profile.** Run `forge agent-profile preview`, then
   `forge agent-profile register`
   ([prepare a durable profile](../../durable-execution.md#prepare-a-durable-profile)).
   Registration is not selection, and selection is not dispatchability.
4. **Select it explicitly per Check.** Use new Check Template pins, or migrate an existing
   eligible Check with `forge record check-profile-migrate`. That command is preflighted
   through the release gate, and its history is shown by `check-profile-migrations`.
   Historical pins are never rewritten.
5. **Stage the rollout on new Assignments only.**
   - Watch `forge dispatcher durable-launch-status`, `assignment-status`,
     `assignment-transcript` and `assignment-info`.
   - Publication is not automatic start.
   - Never redirect a live or uncertain Assignment.
6. **Diagnostics to expect.** These are documented in the
   [operator guide](../../durable-execution.md#operate-durable-assignments):
   - `durable-launch-*` failure codes;
   - `durable-runtime-owner-active`, `-fence-unproven`, `-proof-unmatched`;
   - `durable-launch-ungranted-unproven`;
   - `durable-components-incompatible` / `-unavailable`;
   - `durable-model-auth-unavailable`;
   - `runtime-purge-*`.

## Rollback procedure

1. Run `forge dispatcher durable-launch-disable --operation-id <id> --reason '<text>'`.
   This sticky, immutable rollback stops **new** durable launches only.
2. **Existing durable Assignments keep the durable backend** for status, transcripts,
   follow-ups, automatic same-attempt recovery (from host proof) and retirement. Nothing
   moves to Herdr, and no pin or Workspace selection is rewritten.
3. **Herdr Assignments** keep Herdr cleanup. **Grilling** stays on Herdr.
4. **To continue a Task on Herdr:** retire its durable Assignment safely, then make an
   explicit compatible pin selection. There is no automatic backend switch.
5. **Uncertain work keeps its capacity.** Resolve it only through the documented paths:
   - effect resolution;
   - retirement from host proof;
   - for an unproven state, investigation. Never delete markers or receipts.

## Retained runtime storage and transcripts

- **Runtime stores** live outside worktrees at
  `<workspace-root>/.forge/assignment-runtimes/<digest>` with owner-only access. A
  Revision switch keeps them. They are served only by components whose hash matches the
  Launch, or refused with `durable-components-unavailable`.
- **Retirement keeps history.** `forge dispatcher assignment-purge-runtime` deletes a
  runtime only when all of these hold:
  - the Assignment is closed and fully reconciled;
  - the Dispatch Hold is cleared;
  - no live owner remains;
  - the directory holds only recognized lifecycle files.

  Purge is irreversible. Forge's audit records stay, and transcripts afterwards report
  `runtime-history-purged`.
- **Transcripts** are sensitive runtime data. Same-user tools can read the key file, and
  tool output is stored unredacted.

## Known limitations carried into any future release

- **Trust boundary.** The system runs under a trusted local host. There is no
  hostile-agent isolation. Same-user processes can escape supervision.
- **No guarantees from persistence or leases alone.** Neither gives exactly-once execution
  or proof that a process died. Interrupted shell effects are never replayed.
- **Follow-up limitation.** A follow-up committed after the original answer settled,
  followed by a crash, blocks recovery. Retirement remains available.
- **Proof can be impossible.** A host killed before proving quiescence keeps capacity
  visibly blocked by design.
- **Power loss.** Process-crash tests do not establish durability against physical power
  loss.
- **No provider retry.** Retries are disabled by design. A transient provider or network error
  leaves the Assignment with Missing Outcome; recover with an operator follow-up or retirement.
- **Outcome submission depends on the profile.** Real runs showed `gpt-4.1` ending with prose,
  or claiming a submission it never made, when the profile was vague. Forge never infers
  completion. Profiles must give an exact submission command (see the harness profiles in
  `crates/cli/tests/durable_real_provider.rs`) and reviewers should get only `read` and `bash`.
