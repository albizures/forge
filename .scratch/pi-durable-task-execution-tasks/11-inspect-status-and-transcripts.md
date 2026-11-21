# T11 — Inspect durable execution and transcripts without resuming work

## Delivered outcome

Operators can inspect live or retained durable Assignment status and paginated committed transcripts through Forge CLI/RPC without starting a runner or scheduler.

## Context and scope

Parent: `../pi-durable-task-execution-spec.md`; AC12–14, AC23. Build on T10. Extend existing Assignment inspection using app read interfaces, rpc wire types, server read adaptation and cli presentation. Keep structured output separate from diagnostics and assess compatibility.

Expose backend/runtime ownership identity, lease state, known activity, missing Outcome, uncertainty, capacity and retirement blockers. Paginate committed transcript entries with stable identity/order and explicit continuation semantics during live appends. A stopped/missing runner remains inspectable through a read-only path; an unavailable or corrupt store produces diagnostics without writable repair or resurrection. Respect owner-only runtime access and avoid logging capability/model credentials or copying secret paths unnecessarily.

## Acceptance checks

- Running, crashed, retired and missing-Outcome Assignments are inspectable through CLI and RPC.
- Inspection causes zero submissions, model calls, tool calls, lease grants or scheduler starts.
- Readers beside an active writer do not repair files or acquire writable ownership.
- Pagination preserves committed entry identity/order and reports unavailable history truthfully.

## Verification and evidence

Test both interfaces with active writes, stopped runners, corrupt/unavailable runtime storage and retained closed Assignments. Assert zero execution side effects and no secret fixture values in output/diagnostics. Add help/examples and command-shape tests. Retain sample redacted structured output and commands.

## Dependency

Depends on T10. Code Stack predecessor: T10.
