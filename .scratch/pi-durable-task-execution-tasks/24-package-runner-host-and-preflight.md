# T24 — Package the runner host and preflight matched components at launch

## Delivered outcome

The production launch path from T23 runs entirely from a matched, attested Forge Revision installation: the runner bundle, the `forge-runner-host` binary and the pinned Node. Missing or incompatible components fail launch preflight with an actionable diagnostic. Retained Assignments remain inspectable and retirable after a Revision switch.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC19 and AC21. This is a correction Task for T18 finding **B1** (packaging part) and the parts of **M3** that packaged launch needs ([review](../../docs/research/pi-durable-t18/README.md#m3--major-ac19-met-only-in-part)). The responsible deliverable is T15.

Current gaps:

- `forge-runner-host` is not part of the runner bundle or the Forgeup Installation.
- Durable launch preflight (`crates/server/src/durable_profiles.rs`) checks only the profile and the key file; it does not check runner components.
- The Launch component hash is not mapped to the bundle tree hash. Because the effect inspector must match the Launch hash, retirement and effect resolution for retained Assignments are blocked after a Revision switch, and `runner resolve` cannot find them by component hash.

Changes:

- Include and attest `forge-runner-host` with the matched runner. The daemon must locate it from the selected Installation, never from `PATH`.
- Add launch preflight for every matched component.
- Record a component identity on the Launch that resolves to the original installed components. Retained-Assignment operations then keep using their original matching code, or refuse explicitly when it is gone. Do not silently switch to incompatible code.
- Update `crates/docs/management-protocol.md` for the managed launch inputs (T18 minor 9), and assess the protocol revision (CONTRACT-3).

Do not add a model-credential mechanism here. That is T18 finding M4, which is out of scope for this Task.

## Acceptance checks

- A Forgeup-installed Revision dispatches a durable Assignment through T23's production path, using only its packaged components. This needs a manual daemon with a key file or the test-only gate control.
- Launch preflight rejects a missing, tampered, mismatched-protocol or wrong-Node component before any allocation.
- After switching Revisions, a retained Assignment launched under the previous Revision can still be inspected and retired with its original components, or refuses with an explicit diagnostic. It never runs under the new components.
- `PATH` and `npm` cannot select any execution component.

## Verification and evidence

Run a real packaged install → dispatch → Outcome test. Add negative tests for each tampered or missing component, and a test of a Revision switch with a retained Assignment that is then retired.

Run all four repository gates, runner typecheck and the Node tests. Record the results in `docs/research/pi-durable-t24/README.md`.

## Dependency

Depends on T23. Code Stack predecessor: T23.
