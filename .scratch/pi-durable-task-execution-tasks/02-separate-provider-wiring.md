# T02 — Separate provider selection from Herdr-specific wiring

## Delivered outcome

A green expand-phase refactor that permits multiple adapters without changing existing Herdr execution or cleanup behavior.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1, AC2, AC17, AC21. Build on T01's contract. Start with `crates/app/src/lib.rs`, server adapter selection/capability delivery, profile preflight and workspace dispatch. Follow ARCH-1/2/3: policy in app, wire contracts in rpc, provider adaptation in server.

Separate common capability delivery, adapter selection and new-execution preflight from Herdr flag construction, socket prerequisites and container binding. Preserve selection by immutable Assignment provenance for historical operations. Add explicit unsupported-provider handling rather than a default adapter. Keep durable launch disabled; this Task establishes a seam, not a partial production backend.

## Acceptance checks

- Existing Herdr profiles provision, observe and retire through unchanged ownership checks.
- Missing source files do not newly block historical recovery or retirement.
- Unknown backends fail closed; current selection cannot redirect historical cleanup.
- No Herdr binding guard, one-shot launch or capacity invariant is weakened.

## Verification and evidence

Run focused server dispatch, profile, capability-delivery and recovery regressions, including provider identity mismatches and retained closed Assignments. Demonstrate a fake alternate adapter selected through the new seam without Herdr-only validation. Record commands/results and remaining full-workspace gates.

## Execution evidence — complete

Implemented the expand-phase provider seam:

- `crates/app/src/lib.rs`: added the provider-neutral `validate_provision` port for new-execution checks before capability delivery. Historical operations do not call it.
- `crates/server/src/lib.rs`: the server factory and capability-delivery decorator now wrap provider-neutral factory/adapter interfaces rather than concrete Herdr types. Matched CLI resolution, protected secret delivery and bootstrap text remain server-owned.
- `crates/server/src/herdr.rs`: Herdr implements the factory port and owns Pi flag/source preflight, socket probing and exact retained Workspace verification before capability issuance. Unknown providers (including `pi-durable`) explicitly reject without fallback. Existing provision intent, one-shot launch, observation and retirement ownership checks remain intact.
- `crates/server/src/provider_wiring_tests.rs`: an injected alternate provider passes its own preflight and receives common bootstrap/capability delivery without a Herdr binding or compatible Pi CLI policy. Its denied launch issues no capability. Observation/recovery/retirement neither reload sources nor issue capability files. Production unknown-provider selection rejects before storage access or capability issuance.
- Updated existing test composition to use the provider factory port; no persistence or RPC schema changed, and no dependencies were added. The app trait addition has a default implementation for existing adapters.

Verification on the final code snapshot, from the repository root:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

All passed. Workspace tests: **565 passed, 0 failed, 1 ignored**; server library: **108 passed, 0 failed, 1 ignored**. Coverage includes both new provider-seam tests and existing profile drift/redaction, protected capability delivery, exact provider identity loss/mismatch, source-independent historical recovery/retirement, closed retained resources, crash-window no-replay, Workspace binding mutation guards and original-owner recovery after restart. Tests ran with default parallelism and isolated fixtures. Initial fixture schema/path mistakes and a test-helper Clippy warning were corrected before the final green run.

Review: task/spec and coding-standards self-review found no introduced violations or outstanding T02 findings. This is not the independent T18 review.

Boundaries: production remains explicitly Herdr-only; durable launch is disabled. This is an injectable provider seam, not durable dispatch readiness. Existing application/store Workspace admission and binding guards are deliberately unchanged; backend-specific admission and complete scheduling integration remain successor work (T03/T07/T16). No live configuration, profiles, Assignments, Forge Records or publication changed. All four required Rust/repository gates ran; the ignored opt-in real Pi provider probe, real-provider release cycle and matched runner/packaged recovery gates remain unrun and are not claimed as readiness.

## Dependency

Depends on T01 and builds on its findings. First code-producing Task; proposed code Stack root, with destination resolved only if this filesystem plan is later published.
