# T24 — Package the runner host and preflight matched components

Corrective Task for T18 **B1**, packaging part, and the launch-relevant part of **M3**
([review](../pi-durable-t18/README.md#m3--major-ac19-met-only-in-part)). The responsible
deliverable is T15. Controlled deliverable: new durable launches stay **withheld by
default** behind the T16 release gate. A Forgeup-managed daemon still has no
model-credential mechanism (T18 **M4**, out of scope).

## Problem

- `forge-runner-host` was not part of the runner bundle or the Installation. A managed
  daemon therefore composed no durable launch (T23: "a managed daemon gets `None` until
  T24").
- Launch preflight checked only the profile and the key file, not the runner components.
- The Launch `component_hash` (the runner sources' `contract.ts::componentHash`) was not
  mapped to the bundle tree hash. After a Revision switch, the new Revision's inspectors
  refused the retained Launch, so retirement and effect resolution were blocked.
  `forgeup runner resolve` also could not find the Revision by the Launch hash.
- `crates/docs/management-protocol.md` did not list the managed durable inputs
  (T18 minor 9, CONTRACT-3).

## Delivered

### Shared bundle layout (`forge_app::durable_execution::bundle`)

One definition of the fixed component locations (`node/node`,
`host/forge-runner-host`, `runner/src/check-entry.ts`, the three inspectors, the lock)
and the pinned Node version (24.18.0). Forgeup and the daemon both use it. A unit test
keeps `PINNED_NODE_VERSION` equal to `runner/package.json` `engines.node`.

### Host compatibility (`forge_runner_host::entry`)

The `forge-runner-host` binary and test re-executions share one entry. Its
`--compatibility` flag prints `{"protocol", "runtime_schema", "forge_revision"}` (the
commit, with `-dirty` for a modified checkout) and supervises nothing.

### Bundle schema 2 (`runner/package-bundle.mjs`)

- `--runner-host PATH` is required. The packager checks that the host reports protocol 1
  and the same commit, then copies it to `host/forge-runner-host` (mode 0755).
- The manifest becomes `schema: 2` and records `runner_component_sha256`, computed by the
  bundled sources with the bundled Node. This is exactly the `component_hash` every Launch
  carries.

### Forgeup (`crates/forgeup`)

- **Verification** accepts schemas 1 and 2. Schema 2 additionally requires:
  - the host is a regular file;
  - the host reports the manifest's protocol and this Installation's Revision;
  - the recomputed component hash equals the manifest's.

  `InstalledRunner` gains optional `host_path` and `component_sha256`, so T15 registry
  entries still load. A schema 1 Installation is inspection-only (`launchable() ==
  false`).
- **`runner resolve --component-hash`** accepts either a bundle tree hash or a Launch
  component hash. `runner verify`/`resolve` print the host and component hash.
- **`managed_durable_arguments(paths, registry, installation)`** chooses what
  `daemon start` passes:
  - **schema 2:** `--durable-runner-bundle ROOT`, plus `--durable-retained-runner-bundle
    ROOT` for each other registered schema 2 Revision that verifies, with a distinct
    component hash. A retained bundle that fails verification is left out and recorded in
    `forgeup.log`.
  - **schema 1:** the T15 `--durable-node`/`--durable-effect-inspector`.
  - **No bundle:** nothing.
- `register_at` and `runner_for_component_at` are public for isolated callers.

### Daemon (`crates/server`)

- **Managed inputs.** `--durable-runner-bundle` and the repeatable
  `--durable-retained-runner-bundle` are managed-only.
  - The bundle cannot be combined with `--durable-node`/`--durable-effect-inspector`.
  - Retained bundles require the runner bundle.
  - All paths must be absolute.
  - `DaemonConfig::managed` derives `durable_execution` and `durable_inspection` from the
    bundle with `Execution::from_bundle`. It adds `durable_retained` and keeps
    `durable_model_key_file: None` (M4).
- **Launch preflight (`Launcher::preflight`).** It runs from `validate_profile`/
  `validate_preflight`, `Adapter::probe`, `validate_provision` and `provision`, before the
  single-launch claim. The application calls `validate_preflight` and `probe` before it
  allocates an Assignment, worktree or capability. The checks:
  1. every component is present at its canonical path;
  2. the host and Node files are unchanged since composition (device, inode, size and
     mtime fingerprint);
  3. `node --version` is exactly `v24.18.0`;
  4. `host --compatibility` reports this daemon's runner protocol, runtime schema and
     Forge Revision;
  5. the runner sources still attest the composed component hash.

  Each failure is `durable-components-incompatible` (adapter `InvalidRequest`) with a
  message naming the component.
- **Component catalog (`launch::ComponentCatalog`).** The daemon's own set and every
  retained set, each attested lazily from its own sources with its own Node. Consumers:
  - `retirement::Controller` (crash and receipt reconciliation inspection);
  - `recovery::Controller::with_catalog`;
  - `OperatorService` (transcripts, effect resolution);
  - `Daemon::with_durable_operator_catalog`.

  Selection rules:
  - An existing Launch is served only by the set whose hash equals its
    `component_hash`.
  - With no match, the operation refuses with `durable-components-unavailable`. The
    message names the hash and `forgeup runner resolve --component-hash`.
  - Recovery selects before any claim, storage access or spawn.
  - The only legacy exception: an inspection-only manual selection that cannot be
    attested at all is used as before, and its inspector still rejects a foreign Launch.
- `compose(database, inspection, execution, retained, key, original)` takes the retained
  sets.

### Documentation

- **`crates/docs/management-protocol.md`:** new *Optional durable-runner inputs* section,
  and the protocol-revision assessment (still revision 1, see below).
- **`docs/development.md#matched-runner-packaging`:** schema 2, host, preflight, retained
  Assignments, resolution.
- **`docs/durable-execution.md`:** release state, launch matrix, install steps,
  preflight, retained Assignments. It also fixes the manual example paths to the real
  layout (`node/node`, `host/forge-runner-host`).
- **`runner/README.md`:** a T24 section.

### Management Protocol assessment (CONTRACT-3)

The Management Protocol stays at **revision 1**. The durable inputs are optional and
managed-only. Forgeup selects them from the Installation's own bundle schema, and a
bundle is packaged for exactly its Installation's commit. A Revision therefore only
receives inputs that its own `forge-server` accepts:

- schema 2: the new inputs;
- schema 1: the T15 inputs;
- no bundle: none.

Revision Identity, the required inputs, `system.status` and lifecycle control are
unchanged. Runner wire protocol 1, the durable runtime schema and the store schema are
unchanged.

## Acceptance checks

| Check | Evidence |
| --- | --- |
| A Forgeup-installed Revision dispatches through T23's production path using only its packaged components | The new `packaged` scenario of `durable_supervised_tracer` (see the list below the table). |
| Preflight rejects a missing, tampered, mismatched-protocol or wrong-Node component before any allocation | `launch_preflight_accepts_matched_components_and_rejects_each_fault_before_allocation` (server, see below). Forgeup's install-time counterparts: `host_protocol_revision_and_component_faults_block_with_actionable_messages` and `schema_2_bundle_attests_its_host_and_the_launch_component_identity` (tampered, missing host). |
| After a Revision switch, a retained Assignment is inspected and retired with its original components, or refuses explicitly; it never runs under the new components | `packaged` scenario, steps 1 and 2. Unit: `retained_launches_select_their_original_components_or_refuse_explicitly` (exact selection, explicit refusal with the hash and Forgeup hint, recovery refusal before any storage access, legacy inspection fallback). |
| `PATH` and `npm` cannot select any execution component | Components come only from absolute Installation paths (`Execution::from_bundle`). Every host, Node and inspector spawn clears the environment. The scenario checks `/proc/<pid>/exe` of host and worker against the Installation paths. Manually, `daemon start` with a fake `node` and `forge-runner-host` first on a minimal `PATH` still launched with the Installation's bundle (below). |

**The `packaged` scenario** proceeds as follows:

1. **Packaging and registration.**
   - Two Revisions are packaged in the bundle layout: the real Node, the real
     `forge-runner-host` built by this Cargo invocation, the shipped runner sources and
     the production dependency closure.
   - The test computes the schema 2 manifest independently.
   - Both Revisions are registered through `forgeup::register_at`, which verifies the
     bundle, installs it and re-verifies the installed copy. The second Revision's
     sources differ by one file, so the component hashes differ.
   - The source bundles are deleted before anything runs.
2. **Composition.** The daemon composition is built from exactly the arguments that
   `forgeup daemon start` passes (`managed_durable_arguments`), parsed by
   `DaemonConfig::managed`, behind the production `ReleaseGate` with the test-only
   controlled eligibility.
3. **First launch.**
   - Dispatch launches with the Installation's host and Node, checked by
     `/proc/<pid>/exe`.
   - The Launch records the Installation's component hash.
   - The worker crashes at the pre-generation boundary.
4. **Switch without the original components.**
   - The first bundle is temporarily tampered, so Forgeup withholds it from the second
     Revision's daemon.
   - Recovery refuses with `durable-components-unavailable`.
   - An actual CLI `assignment-transcript` through that daemon refuses with the code and
     the hash.
   - `owner.json`, the Launch and capacity are unchanged.
5. **Switch with the original components.** With both bundles verified, the second
   Revision's daemon is offered the first bundle:
   - The CLI transcript succeeds (`Available`, non-empty).
   - Ordinary `reconcile_agent_dispatch` recovers the same attempt. The host and worker
     run the **first** Installation's packaged host and Node.
   - The recovered run produces an accepted Outcome. The protected input, Launch and
     attempt are unchanged.
   - CLI `assignment-retire` through the second daemon reconciles through the first
     Revision's effect inspector and releases capacity.
6. **New dispatch on the second Revision.** A new Task dispatched by the second
   Revision's composition runs from **its** packaged host, Node and sources to an
   accepted Outcome.

**The preflight unit test** builds bundle-layout fixtures: real runner sources, a Node
wrapper and a host script. Starting from a positive control, each fault is refused by
`preflight`, `validate_preflight` and `probe` with `durable-components-incompatible`:

- tampered sources, then the same file restored;
- a missing component;
- the host or Node replaced after composition;
- Node v22.19.0;
- a protocol + 1 host;
- a host from another Revision;
- a host that does not answer.

### Additional tests

- **Forgeup:**
  - schema 2 install/re-attest with host and component;
  - tampered and missing installed host;
  - host protocol 2, other Revision, silent host;
  - wrong recorded component;
  - sources edited with a re-declared tree hash;
  - schema/field mismatches;
  - schema 1 remains inspection-only and T15 registry entries still deserialize;
  - Launch-hash resolution;
  - the daemon arguments: selected plus retained, schema 1 legacy inputs, and a tampered
    retained bundle withheld and diagnosed. The arguments are computed while the
    registry lock is held.
- **Server:** `bundle_root_derives_every_component_from_the_fixed_layout`, and managed
  configuration assertions:
  - the bundle derives execution, inspection and retained sets;
  - combining the bundle with legacy inputs, retained bundles without a runner bundle,
    and a relative root are each rejected;
  - no model key file is configured.

### Defect found by the manual run and fixed

Run against the real CLI, `forgeup daemon start` hung forever. It holds the registry
lock, and the first version of `managed_durable_arguments` reopened the registry and
waited on the same `flock` through a second open file description. The unit test had
called the function without holding the lock. The function now takes the caller's
loaded `Registry`, and the Forgeup test computes the arguments while holding the lock,
as `daemon start` does.

### Test-only adjustments

- **`duplicate_provision` (accepted scenario)** now waits for the live driver to reconcile
  its one-shot `provision-intent` marker before taking its "before" snapshot. Preflight in
  the fresh composition's `provision` widened an existing race between that snapshot and
  the driver's post-admission reconciliation. The asserted behavior is unchanged.
- **Tracer re-executions** of `--supervisor` now call `forge_runner_host::entry::run`, so
  the test host also answers `--compatibility`.
- **Embedded daemon CLI calls** in the packaged scenario run on Tokio's blocking pool.
  Calling the blocking CLI from the task that spawned the new embedded server could leave
  that server unpolled (Tokio LIFO slot) and time out.
- **`[profile.dev.package.sha2] opt-level = 3`** in the workspace manifest, because
  attestation hashes several hundred MB in tests. There is no behavior change.

## Manual packaged run (real packager and Forgeup CLI)

Run in an isolated `FORGEUP_HOME`, with debug builds of this checkout:

1. `node runner/package-bundle.mjs --out … --forge-commit <HEAD> --runner-host
   target/debug/forge-runner-host` produced a 296 MB schema 2 bundle. Its manifest
   records tree `40820444…0dc0`, lock `69dfc738…bdf1` and component `5bb84565…5182`.
   `npm ci` used the local cache.
2. `forgeup register --runner-bundle`, then `use`. The source bundle was deleted, then
   `runner verify` printed host, entry, tree and component from the Installation.
3. `runner resolve --component-hash 5bb84565…5182` (the Launch hash) found the Revision.
   An unknown hash refused with "not redirected".
4. `env -i … PATH=<fake node and forge-runner-host>:/usr/bin:/bin forgeup daemon start`
   reported `started`. The managed `forge-server` command line carried
   `--durable-runner-bundle <Installation>/runner-bundle`, and `daemon status` reported
   `running`.
5. After `daemon stop`, appending one byte to the installed `host/forge-runner-host`
   made both `daemon start` and `runner verify` fail with "bundle contents do not match
   the attested tree hash". Restoring the bytes made it verify again.

This is not a managed-daemon dispatch: a managed daemon has no model key (M4), and the
release gate withholds launches.

## Gates

`PATH` had the real Node 24.18.0 first (`~/.volta/tools/image/node/24.18.0/bin`), because
the Volta shim breaks `durable_check_bootstrap`.

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass: 908 passed, 0 failed, 1 ignored (pre-existing opt-in probe). All 11 supervised tracer scenarios passed, including `packaged`. |
| `git diff --check` | pass |
| `runner`: `npm run check` (tsc) | pass |
| `runner`: `npm test` | pass: 97/97 |

## Not claimed

- **No real provider.** Every tracer scenario substitutes the runner entry with the
  faux-model worker. In `packaged`, that worker runs the *installed* `check-runner.ts`
  and dependency closure (its `src` and `node_modules` resolve into the Installation).
  The unsubstituted packaged `check-entry.ts` did not run an Assignment here; its refusal
  without bootstrap/profile/key is T22's evidence. Real-provider verification is T19.
- **No `forgeup daemon start` dispatch.** The automated test composes the production
  path in-process, from Forgeup's managed arguments, with the test-only gate control. A
  managed daemon still has no model credential (M4), and launches stay withheld.
- **Test-assembled bundle.** The automated test assembles the bundle in the packager's
  layout. Running `package-bundle.mjs` needs `npm ci`, so the real packager was
  exercised only in the manual run.
- **Stale host detection.** The automated test takes `forge-runner-host` from this Cargo
  invocation's target directory. It rejects a binary reporting another Revision, but
  cannot detect a stale binary from the same dirty Revision. `cargo test --workspace`
  builds it fresh.
- **Preflight depth.** Host and Node replacement is detected by file metadata
  (device, inode, size, mtime), not by rehashing on each dispatch. The dependency closure
  is attested by Forgeup's tree hash at `daemon start`, not at each preflight. Deliberate
  same-user tampering between checks stays out of scope (T15).
- **Retained sets are not launch-preflighted.** A retained set is selected by its runner
  component hash and verified by Forgeup at `daemon start`. Its host, from the earlier
  Revision, must still speak runner protocol 1, which is unchanged. A future protocol
  change needs its own compatibility rule.
- **Effect resolution** through a retained set is covered by catalog selection unit
  tests, not by an end-to-end scenario. Transcripts, recovery and retirement reconciliation
  are covered end to end.
- **M1 is unaddressed.** A retained Assignment whose daemon died mid-execution
  (watchdog-fenced) still has no reconciliation path, whatever components are available.
