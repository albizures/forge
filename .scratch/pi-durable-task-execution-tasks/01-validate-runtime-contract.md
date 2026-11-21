# T01 — Validate the pinned runtime contract and implementation seams

## Delivered outcome

A source-backed implementation contract proving how the selected pi-durable release can satisfy Forge's safety requirements, with runnable bounded probes rather than compatibility assumptions.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`. Read its Notes, Implementation Decisions and AC5–9, AC17–19. This is research/probing, not production dispatch or a second architecture interview. Preserve the approved Rust/TypeScript boundary and trusted-local-host model.

Inspect the exact upstream source and select an exact package release/revision and supported JavaScript runtime. Probe writable/read-only storage opening, scheduler startup, submission identity, replay policy, interruption detection, cancellation, model/tool admission hooks and provider authentication. Identify how every call path—including compaction, nested tools and resumed work—will be covered by authorization. Inspect local process-supervision, profile, Revision-installation and schema-compatibility seams.

Record protocol operation identities, ownership/fencing strategy, lease clock/renewal assumptions, supported resource/authentication matrix and required packaging interfaces. Select concrete technical mechanisms only where they preserve settled policy. Material incompatibilities return for decision; never substitute automatic replay, ambient discovery or weaker retirement guarantees.

## Acceptance checks

- Exact upstream version/source and runtime requirements are recorded.
- Each safety mechanism has an inspected API/host seam and a reproducible probe or explicit blocker.
- Supported initial profile/authentication combinations and unsupported combinations are explicit.
- No production default, profile selection, dispatch or external publication changes.

## Verification and evidence

Run isolated temporary-storage probes, including interrupted unsafe tools and observation beside a writer. Save commands, outputs, dependency identity and caveats in a repository research/contract document. Do not retain secrets. Expected result: a concrete, testable implementation contract for successors, or a reported material blocker.

## Execution evidence

Complete as a research/probing deliverable. See [the pinned implementation contract](../../docs/research/pi-durable-t01/README.md) and its reproducible fixtures: 19 Node probes and 4 Linux host probes pass. The initial read-only storage blocker is resolved by an explicit, hash-pinned derived read adapter; no upstream package or production configuration was silently modified. Provider/tool admission guards, unsafe-effect recovery, committed observation, auth/resource mapping and host ownership mechanisms are probed with the documented limits. Production integration and real-provider release verification remain successor obligations.

## Dependency

None. This is the root of the ordinary dependency chain; it is not a code Stack root.
