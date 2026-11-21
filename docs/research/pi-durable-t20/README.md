# T20 — Durable Task execution: readiness decision and release handoff

## Readiness decision

**WITHHELD.** Do not enable default durable Task dispatch from any Forge Revision built
from this source.

| Basis | Evidence |
| --- | --- |
| Reviewed snapshot | `2097267`: T18 third pass, no open blocker ([report](../pi-durable-t18/README.md#third-pass--2026-11-21-final-snapshot-2097267)) |
| Verified tree | `6cd2e44` (`2097267` plus two documentation files), Forge Revision `6cd2e44f801f6fe5d84330569d5ab74810ba4172` ([T19](../pi-durable-t19/README.md)) |
| Deterministic gates | **Pass:** fmt, clippy, 931 Rust tests twice in a row, 25 real-process tracer scenarios, 99 Node tests, packaging |
| Release gate (Spec AC22) | **UNRUN:** no real-provider Implementation → Review cycle; no managed-daemon launch |
| Open MAJOR | **M4:** a Forgeup-managed daemon has no model-credential mechanism |

A failed or unrun release gate is a withheld result. It is not a conditional success.

The source behaves accordingly:
- `forge dispatcher durable-launch-status` reports `withheld` and
  `new_launches_admitted: false`;
- no shipped command can change that.

This handoff performs no rollout, no profile registration or selection, no pin migration,
no dispatch and no configuration change.

## What passed, and with which components

These identities are from the T19 run:

| Component | Identity |
| --- | --- |
| Runner bundle | schema 2; runner protocol 1; durable runtime schema 2 |
| Runner component hash (carried by every Launch) | `8991977e0e4cb7c4468875f5cbec11ac69403477e14d5378c92a7346a4af28a0` |
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
- a raw OpenAI key file on a **manual** daemon.

Everything else is rejected, fail-closed:
- Grilling;
- any other model or provider, and OAuth;
- Pi settings;
- a managed-daemon credential (M4).

## What must happen before readiness can be granted

1. **Close or formally descope M4.** Either give Forgeup-managed daemons a deliberate
   model-credential mechanism (then re-review and re-verify), or have the operator
   explicitly limit the supported deployment to manual daemons.
2. **Run the real-provider gate.** Follow the steps in
   [T19, "How to run the missing real-provider gate"](../pi-durable-t19/README.md#how-to-run-the-missing-real-provider-gate),
   on the exact Revision to be released, in an operator-authorized, isolated environment,
   and retain a redacted report.
3. **Decide the open T18 minors and test gaps.** Fix them or accept them explicitly
   ([list](../pi-durable-t18/README.md#still-open-non-blocking)).
4. **Implement eligibility.** Only after steps 1–3 pass on that Revision can launch
   eligibility be implemented and reviewed as its own change. Today no shipped command
   grants it, and it must not be added as a side effect of this handoff.

## Conditions that revoke readiness, or require renewed verification

- **Any change** to the implementation, the runner sources, the lock file, the pinned Node
  or pi-durable, the runner protocol, the store schema or the host. The runner component
  hash or Revision then changes, and T18-style review plus T19 verification must be
  renewed on that exact Revision.
- **A different platform, model or authentication mechanism** from the supported matrix.
- **Any failing or intermittent** release-gate scenario that has not been explained.
  `committed_model_authority_expires_during_renewal_outage…` failed once under full load
  and is recorded but not claimed fixed.
- **An operator rollback** (`durable-launch-disable`). It is sticky and immutable for the
  store.

## Operator checklist: deliberate adoption, once readiness is granted

This is listed here for completeness. **Do not perform it while readiness is withheld.**
Every command below exists and is documented in the
[operator guide](../../durable-execution.md).

1. **Install matched components** for the Revision. Use
   `node runner/package-bundle.mjs … --runner-host …`, then
   `forgeup register … --runner-bundle …`, `forgeup runner verify <selector>` and
   `forgeup daemon start` ([install](../../durable-execution.md#install-and-run-the-matched-runner)).
   Keep every older Revision's bundle registered while it still has retained durable
   Assignments, so they run only on their own components (`forgeup runner resolve
   --component-hash`).
2. **Supply the model credential** on a manual daemon with `FORGE_DURABLE_OPENAI_KEY_FILE`:
   an owner-only `0600` key file, kept outside every Managed Worktree. (M4: not possible
   on a managed daemon.)
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
