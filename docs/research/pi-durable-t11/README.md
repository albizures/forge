# T11 — Read-only status and committed transcripts

Controlled T11 deliverable. Production durable dispatch and automatic recovery stay disabled.

## Delivered

- App `durable_execution::status` (identity/lease-history/`View` policy) with a store read in one
  deferred transaction; scope is authorized before any read. No store path, owner process, profile source
  or capability is exposed.
- RPC `assignmentDurableStatus` and `assignmentTranscript` (`rpc/src/assignment_inspection.rs`), plus the
  internal runner wire `rpc/src/durable_execution/transcript_inspection.rs` (exact launch, ordered IDs,
  bounded entries, `unavailable` requires a code and carries no entries).
- Runner `inspect-transcript.ts`/`observeTranscript`: read-only derived reader, `after` entry-ID cursor,
  no Harness, no writes, fixed reason codes. It is part of the component hash.
- Server `durable_authorization/transcript.rs`: canonical runtime mapping, no host lock, no retention
  write, post-I/O binding recheck. Reader is the sibling of the configured matched effect inspector.
- CLI `dispatcher assignment-status` / `assignment-transcript` with help examples, `--json` on stdout only.
- Operator documentation in `docs/assignment-recovery.md`.

## Acceptance evidence

| Check | Evidence |
| --- | --- |
| Running, crashed, retired, missing-Outcome inspectable through CLI and RPC | `durable_check_bootstrap` (final-prose and rejected-then-prose cases): actual CLI → daemon → Node reader against a real runtime, human + JSON, status then paged transcript; repeated after test-controlled `Retired` closure through a fresh operator daemon. Runner-host `status_and_transcript_are_read_only_beside_live_stopped_closed_and_corrupt_runtimes` covers a live supervisor owner, killed supervisor and closed Assignment. |
| Zero submissions, model/tool calls, lease grants, scheduler starts | Readers need no profile, key or capability. Tests assert lease count stays 0, resources/capacity/hold unchanged and `agent_assignment_info` equal before/after. |
| Readers beside a live writer do not repair or take ownership | Node test with an open writer connection; Rust test with a live lock-owning supervisor that stays alive. Runtime bytes are unchanged across reads. |
| Pagination identity/order and truthful unavailable history | Node: live appends between pages yield exactly the committed IDs in order; idle page echoes the cursor. Corrupt/foreign storage → `unavailable` + fixed code, bytes unchanged, no canary disclosure. Rust wire/request validation tests. |

## Gates

`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test
--workspace` 832 passed, 0 failed, 1 pre-existing opt-in probe ignored; runner `tsc --noEmit` and 89 Node
tests pass. Run with a real Node 24 first on `PATH` (Volta shim limitation, unchanged).

## Limits

- The tracer-based missing-Outcome tests copy the test-owned runtime into the canonical protected mapping;
  they do not exercise production dispatch.
- Daemons built without the retained store/inspection components cannot answer; this is reported, not guessed.
- Component hash changed (new reader file): a retained runtime whose Launch records an older hash is refused by the
  reader (`durable-transcript-unavailable` RPC error), with no automatic conversion.
- Transcript content is operator-sensitive; the sample below is synthetic.

## Sample commands and redacted structured output

```sh
forge dispatcher assignment-status asg-1 --workspace owner --json
forge dispatcher assignment-transcript asg-1 --workspace owner --limit 2 --json
forge dispatcher assignment-transcript asg-1 --workspace owner --after 12 --json
```

```json
{"assignment_id":"asg-1","history":"available","reason":null,
 "entries":[{"id":"11","kind":"fixture.message","by_task_id":null,"content":"{\"model\":null,\"data\":{\"text\":\"[redacted]\"}}","truncated":false},
            {"id":"12","kind":"fixture.message","by_task_id":"3","content":"[redacted]","truncated":false}],
 "next_after":"12","more":true}
{"assignment_id":"asg-1","history":"unavailable","reason":"runner-history-unavailable",
 "entries":[],"next_after":"12","more":false}
```
