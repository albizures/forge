# T12 — Recorded, idempotent operator follow-ups (partial)

## Runner delivery increment

`runner/src/followup.ts` delivers one recorded follow-up into the resumed, authorized root conversation. It uses runtime request ID `followup:<operation_id>`, checks SHA-256 content identity, and requires live guarded authority and no unsettled interrupted effect. It creates no root, Continuation or attempt, and a retry returns the existing admission (`replayed: true`). `observeFollowUp` is read-only and reports `unknown` or `accepted`. `serveRecovery` accepts a `host_followup` control on the protected supervisor pipe only after the harness has resumed, and emits `followup-accepted` or `followup-failed`.

Node test: `follow-up delivery is idempotent by operation ID, visible in committed history and needs live authority`. It covers a bad hash, a first delivery, a retry, one extra model call, the text in committed transcript history, and denial after expiry. Runner typecheck and 90 Node tests pass. Cargo workspace: 844 passed, 0 failed on reruns; the first run had one failure, `returned_tool_without_committed_receipt_retains_forge_blocker_across_daemon_restart`, which passed on unchanged reruns without a claimed fix.

Rust transport increment: `SupervisorFollowUp` strict wire type (`host_followup`, payload of operation ID, content, SHA-256 only) and `ControlPipe::deliver_followup`, which validates the request, sends one nonblocking write and fences the pipe on any uncertain send. A new test covers exact round trip, unknown-field rejection, separation from recovery control and five fault kinds. The runner content limit is aligned to Forge's 16 KiB. Gates: 845 Rust passed, 0 failed; clippy clean; runner typecheck and 90 Node tests pass (real Node 24 on PATH).

Still missing: the Rust daemon driver that sends `host_followup` and persists delivered/unknown state, plus queue-while-effect-unresolved evidence through the real supervisor.


Controlled T12 increment. **Delivery into the runtime conversation is not implemented**; production durable dispatch and automatic recovery stay disabled.

## Delivered

- Store schema revision 23: `durable_assignment_followups` (immutable, undeletable, per-Assignment sequence, SHA-256 content identity, JSON binding).
- App `durable_execution::followup` (`Request` with `deny_unknown_fields`, validation, `Receipt`, `Repository`); store implementation records in one immediate transaction.
- RPC `assignmentFollowUp`; `assignmentDurableStatus` now lists recorded follow-ups. CLI `dispatcher assignment-followup` (`--operation-id`, `--message` or `--message-file`, `--json`).
- Operator docs in `docs/assignment-recovery.md`.

## Acceptance mapping

| Check | Evidence |
| --- | --- |
| Dropped reply then retry creates one follow-up | store tests (including four concurrent retries and reopen) and actual CLI→daemon retry (`replayed: true`, same timestamp) |
| Visible with operation identity | listed by actual `assignment-status` (human and JSON). **Not** in the committed runtime transcript, because nothing is delivered yet |
| Revoked rejects; uncertainty does not bypass | revoked, stale generation, unconfirmed admission, foreign Workspace, and all five blocking resource kinds are rejected, then allowed once retired; a revoked retry still replays |
| Reviewer "use another commit" keeps evidence | rebinding fields rejected at parse (store and CLI); stored attempt evidence and binding unchanged |
| Missing Outcome receives safe intervention without completion | store and real-runtime CLI test: recorded, attempt still open, diagnostic and capacity retained, transcript entries unchanged |

## Not done

- Rust-side delivery driver, persisted "delivered/unknown" states and real-supervisor composition. The runner-side idempotent submit and committed transcript entry are now done (see above).
- No unauthorized tool/model call assertions beyond: recording touches only Forge's store.
