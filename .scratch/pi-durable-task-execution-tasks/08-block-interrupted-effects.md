# T08 — Retain interrupted-effect uncertainty without automatic replay

## Delivered outcome

A crash after an external effect but before result persistence creates a durable inspection blocker instead of rerunning the command or allowing the model to immediately repeat it.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC5, AC7, AC9, AC10, AC12. Build on T07. Arbitrary `bash` is not replay-safe, even if a surrounding submission can be deduplicated.

Persist sufficient intent and tool/effect identity to reconcile interrupted execution. Translate runtime interruption into Forge-visible uncertainty before recovery can schedule more model/tools. Establish a bounded, explicit operator inspection/resolution path preserving accepted results, ownership guards and audit. Resolution is not permission to replay and does not silently adopt dirty commits or clear unrelated holds. Any truly replay-safe automatic operation needs explicit rationale and tests.

## Acceptance checks

- An external mutation followed by abrupt runner death occurs once, not twice.
- On reopen, neither tool replay nor a fresh model request bypasses the blocker.
- Resolution rejects wrong scope, conflicting retry and unsafe ownership conditions.
- Tracked, staged and untracked work survives unchanged; existing retained-work rules remain authoritative.
- Unknown external outcomes remain unknown until supported resolution.

## Verification and evidence

Use a controlled mutation endpoint and shell process with synchronization immediately before/after effect and result commit. Test interruption around push/publication analogues, replay-safe versus unsafe calls and daemon restart. Count actual mutations, compare worktree/index bytes and inspect retained audit/blockers. Deliver deterministic regression evidence and documented resolution semantics.

## Completion evidence

Completed at the controlled T08 boundary. [Implementation, acceptance evidence and
reviews](../../docs/research/pi-durable-t08/README.md#completion-live-resolution-and-controller-fencing)
cover protected committed intent, runner/controller execution fences, atomic Forge
retention, live/closed audited public resolution, exact retries, actual Bash fault
boundaries and owned supervisor restart rejection. All four repository gates pass:
682 Rust harness tests, six supervised scenarios and 52 Node tests, plus runner
typecheck (one pre-existing opt-in Rust probe ignored).

Live resolution retains the original attempt and effect resource as an execution
fence; it is not safe writable recovery. No external audit repairs a runtime
receipt or permits tool/model replay. T09 may proceed. Safe retirement (T09),
authorized writable recovery (T10), automatic rollout and packaged/real-provider
verification remain their later owning deliverables; production durable dispatch
and automatic recovery remain disabled.

## Dependency

Depends on T07. Code Stack predecessor: T07.
