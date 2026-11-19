# T07 — Execute one Check through authenticated headless dispatch

## Execution status

Complete: [supervised tracer, acceptance evidence and verification](../../docs/research/pi-durable-t07/README.md).
The six parallel controlled scenarios and all repository/runner gates pass.
Production durable dispatch and automatic recovery remain disabled; this completes
only T07's explicit local tracer, not rollout or later safety Tasks.

## Delivered outcome

An explicitly enabled test Assignment goes end to end from Forge dispatch through the durable runner, authenticated bootstrap and accepted Outcome, without Herdr or the Pi CLI.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC1–5, AC13, AC17. Build on T06. Integrate app dispatch, protected capability delivery, server adapter wiring and runner tools. Use existing `forge assignment bootstrap` and Outcome commands, not replacement model-facing lifecycle tools.

Preserve Managed Worktree preparation, parent/dependency/mode/capacity gates, Check pin selection, attempt/evidence binding and freshness validation. Deliver only the short bootstrap before trusted instructions are obtained. Durable readiness must not require a Herdr Workspace binding. Keep default production launch disabled and automatic recovery unavailable until later safety slices land.

Distinguish runtime answer/idle/done from accepted Outcome. Persist and expose missing-Outcome diagnostics without passing a Check, creating a Continuation or launching a successor.

## Acceptance checks

- A controlled eligible Check boots in the bound worktree and submits an authenticated accepted Outcome.
- Failed/uncertain bootstrap stops work; no Herdr resource or Pi CLI invocation occurs.
- Wrong/stale/revoked Outcomes reject under existing rules.
- Final prose without an accepted Outcome leaves unresolved work and reserved safety state as applicable.
- Manual, parent-gated and dependency-blocked Tasks remain ineligible.

## Verification and evidence

Build a deterministic local tracer with a fake model and actual runner/CLI bootstrap path. Assert external call counts, exact identities, outcome evidence and missing-Outcome behavior. Retain Herdr/Grilling regressions. Record commands/results; real-provider verification is reserved for T19.

## Dependency

Depends on T06. Code Stack predecessor: T06.
