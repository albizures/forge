# T15 — Matched runner packaging and attestation

Controlled deliverable. Production durable dispatch and automatic recovery remain disabled.

## Delivered
- `runner/package-bundle.mjs`: builds a bundle from the pinned Node 24.18.0 executable, runner sources, `npm ci --omit=dev --ignore-scripts --no-bin-links` closure and the derived read-only adapter (generated inside the bundle's own dependency copy; its hash equals the development one, `b7801ce1…62aa`). Writes `forge-runner-bundle.json` (schema, runner protocol 1, Node version, Forge commit, `package-lock.json` hash, content tree hash).
- `forgeup::runner`: verifies a bundle (manifest, commit equals the Revision's, protocol, fixed component locations, lock hash, recomputed tree hash, no symlinks/special files, bundled Node `--version` with a cleared environment), copies it into the immutable Installation and re-verifies the installed copy. `Installation.runner` is an optional, backward-compatible registry field.
- `forgeup register --runner-bundle`; `forgeup runner verify SELECTOR`; `forgeup runner resolve --component-hash H`; `forgeup daemon start` fully re-attests and passes the Installation's Node/inspector as explicit `--durable-node`/`--durable-effect-inspector` (new optional, both-or-neither, absolute managed server flags). Ordinary selection only does a cheap presence check.
- `forgeup remove` refuses to delete a runner-bearing Revision while its state exists unless `--purge-state`; resolution by component hash fails closed rather than falling back to another Revision.
- Operator documentation: `docs/development.md#matched-runner-packaging`.

## Management Protocol assessment
Unchanged (revision 1): lifecycle protocol and Revision Identity are untouched; the two daemon flags are optional and passed only for a Revision whose attested bundle was packaged for the same commit. Runner wire protocol 1, runtime schema and store schema are unchanged.

## Evidence
- 8 new forgeup tests in isolated directories: valid install/re-attest from the installed copy (source deleted); tampered entry, extra file, missing inspector, wrong commit, protocol 2, lock mismatch, Node-version mismatch and tree mismatch each block with an actionable message; symlinks and unknown manifest fields rejected; registration idempotent, different bundle conflicts, original component resolvable and unknown hash fails closed, removal retained then explicit purge; invalid bundle leaves no Installation; Revision without a bundle cannot be verified as matched.
- Manual packaged run (real bundle, 244 MB, isolated `FORGEUP_HOME`): packaged → registered → `use` → `runner verify` (tree `37fcc62f…ea09b`, lock `69dfc738…bdf1`); `daemon start` with a minimal `PATH` launched `forge-server` whose command line carried the Installation's matched Node and inspector; a fake `node` first on `PATH` did not change the selected paths; appending a byte to the installed `main.ts` made both `runner verify` and `daemon start` fail with "bundle contents do not match the attested tree hash"; re-registration with `--replace` restored it; `remove` and an unknown `resolve` hash both refused.
- Packaged runner under the real supervisor (`forge-runner-host` with the bundle's Node and `main.ts`, no grant, closed stdin): it owned the runtime, printed its protocol/runtime schema/component hash, and the independent watchdog retained unverified-cleanup diagnostics before the `timeout` I applied expired. This is a packaged start/ownership/retention demonstration only — not a full dispatch/bootstrap/Outcome scenario from the packaged Revision, and the retained-uncertainty files were left in a scratch directory.

## Gates
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check` clean; runner `tsc --noEmit` clean; `cargo test --workspace --no-fail-fast` with a real Node 24 first on `PATH`: 871 passed, 0 failed, 1 pre-existing opt-in probe ignored. Node tests were not rerun (runner sources unchanged).

## Not claimed
- The packaged bundle is not exercised by the supervised Check scenarios (T19); no real-provider run.
- The runner's internal component hash (Launch) and the bundle tree hash are separate identities; Forge does not yet map one to the other, so `runner resolve` takes the tree hash. Linux x64/Node 24.18.0 only; no release signing or download distribution.
- Same-user filesystem tampering between verification and exec is out of scope.
