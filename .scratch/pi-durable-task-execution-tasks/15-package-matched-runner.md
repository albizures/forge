# T15 — Distribute and attest matched runner components with Forge Revisions

## Delivered outcome

A Revision Installation supplies the exact runner/runtime dependencies expected by Forge, and incompatible or missing components fail preflight without selecting arbitrary PATH/npm installations.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC5, AC17–19, AC21. Build on T14 and T01's packaging findings. Route through Forgeup installation/selection, build identity, server runner resolution and the versioned runner handshake. Keep Forgeup lifecycle separate from Record execution policy.

Ship the pinned pi-durable release, locked JavaScript dependencies and declared JavaScript runtime requirements as matched components. Validate executable/component identity and protocol compatibility before new capability issuance or launch. Preserve sufficient original runtime identity and compatible cleanup behavior across Revision switching and retained stores; never reinterpret persisted runtime storage through an incompatible release. Assess Management Protocol changes separately from storage/wire changes.

## Acceptance checks

- An isolated installed Revision can resolve and run its matched component.
- Missing/mismatched runtime, protocol or dependencies block new launch with actionable redacted diagnostics.
- Arbitrary global npm/PATH runner changes do not alter selected execution.
- Switching Revisions cannot redirect an existing Assignment to incompatible recovery/cleanup code or silently rewrite its store.

## Verification and evidence

Test installation/registration/selection in isolated Forgeup state, missing files, tampered/mismatched components and retained-store compatibility failures. Run a packaged deterministic dispatch/bootstrap and cleanup demo, not only a development-source runner. Retain dependency manifest/lock identity, packaging commands and observed failure behavior.

## Dependency

Depends on T14. Code Stack predecessor: T14.
