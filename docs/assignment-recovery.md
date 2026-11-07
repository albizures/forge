# Assignment recovery operator handoff (Spec #568)

This runbook completes the investigation/recovery documentation for safe launch
and acknowledgement (Spec #567). It is a source-level handoff, not evidence of a
published release, installed upgrade or production recovery. The parent Spec's
completion remains an explicit workflow action.

## Trust and upgrade boundary

Back up the Forge database using the normal upgrade policy before opening it
with the new binaries; existing databases migrate automatically. Preserve
Assignment, resource, audit and worktree evidence. Do not edit SQLite, adopt
lookalike provider resources, delete worktree claims or launch a replacement to
bypass a recovery blocker.

The operator HTTP/RPC surface is **trusted and unauthenticated**. Restrict daemon
access to trusted operators. Rejecting agent credential-shaped requests is input
validation, not authentication. An Assignment Capability grants only the agent's
scoped operations; it is not operator authority. Diagnostics exclude Task/profile
prompts, capability values and secret-file paths; treat provider identities and
operational evidence as sensitive nonetheless.

## Investigate before changing state

```sh
forge agent-dispatch assignment-info <assignment-id>
forge agent-dispatch assignment-info <assignment-id> --json
forge agent-dispatch status --project <slug>
forge worktree info <task-id>
forge worktree doctor
```

Inspection shows persisted Task/Assignment state, launch and acknowledgement
facts/deadline, Dispatch Hold/reason, resource identities/retirement, recovery
blockers, capacity reservation and any unknown-resolution audit. It works for
closed Assignments. It does not contact Herdr: a missing timestamp or retained
identifier is not proof of absence or current ownership. Investigate the actual
provider resources and external processes separately; retain evidence without
copying capabilities or prompt contents into logs or tickets.

A missed acknowledgement deadline retains the Assignment and capacity. Clearing
the hold cannot replay a launch or prompt; reconciliation may reapply the timeout
hold. Late acknowledgement can still make the existing Assignment Working and
allow its Task to start without clearing that hold. Do not retire an otherwise
healthy agent just because its acknowledgement was late.

## Revoke and clean up known resources

```sh
forge agent-dispatch assignment-retire <assignment-id> --json
forge agent-dispatch assignment-info <assignment-id> --json
```

Retirement installs a Dispatch Hold and durable cleanup evidence, closes/revokes
the Assignment and removes its subscriptions **before** provider cleanup. It
checks retained ownership identities before destructive requests and verifies
agent/tab/pane absence afterward. It retains Git/worktree evidence and does not
delete the shared operator-selected workspace. Revocation cannot undo external
actions or prove that an external child process is dead.

If cleanup fails, identities mismatch, resources survive, or the provider is
unavailable, stop and investigate. Closed does not mean capacity-free: unresolved
known resources, retirement-pending evidence and provisioning uncertainty survive
restart and reserve capacity. Correct the provider/endpoint problem and retry the
same `assignment-retire`, including for an already closed Assignment. Do not
bypass ownership checks or treat a successful mutation alone as verified absence.

## Resolve unknown resources only after external safety checks

A crash may leave a provisioning-intent marker without the resource identity
Forge would need to clean it up. Known-resource retirement alone does not resolve
this uncertainty. First revoke the Assignment, verify known-resource cleanup,
investigate unknown resources and make the old agent and any external children
unable to act. Remove unknown resources or otherwise make them safe. Only when
both statements are actually true:

```sh
forge agent-dispatch assignment-resolve-unknown <assignment-id> \
  --confirm-old-agent-cannot-act --confirm-unknown-resources-safe --json
forge agent-dispatch assignment-info <assignment-id> --json
```

Both independent confirmations are mandatory; an open/unrevoked Assignment is
rejected. An incorrect assertion risks duplicate agents and concurrent external
work. Confirmation is a deliberate operator safety override, **not proof of
process death or an authenticated human identity**. The immutable audit retains
the timestamp, authority boundary and confirmation text across restart; repeated
calls preserve the original audit.

Only provisioning-intent uncertainty is resolved. Known resources,
retirement-pending and other blockers remain untouched and reserve capacity until
separately resolved. The Dispatch Hold remains, too. Inspect again rather than
assuming the command's success means recovery is complete.

## Clear the hold separately; eligibility is not a launch guarantee

After the previous Assignment is closed and all known and uncertain resources
are safely resolved, review Task state, dependency readiness, Project capacity,
workspace and worktree health. For an unassigned manual In Progress Task that
should run again, explicitly use `forge agent-dispatch task-reopen <task-id>`;
retirement does not complete or automatically reopen the Task.

```sh
forge agent-dispatch resume <task-id>
forge agent-dispatch status --project <slug>
forge agent-dispatch reconcile
```

`resume` only clears the operator-controlled hold. It does not resolve blockers,
prove agent absence, redeliver a prompt or itself authorize replacement. Its JSON
`resolves_recovery_blockers: false` and `authorizes_replacement: false` describe
operation semantics, not a live eligibility verdict. Replacement still requires
normal dispatch checks, including an eligible Open Task and no unresolved old
Assignment/resource evidence. Inspect the resulting Assignment; never create a
manual duplicate to work around withheld capacity.

## Verification handoff and remaining limitations

The authoritative verification outcome is Forge Task #583, replacing #579's
**completed-with-findings**, not-passing sign-off. #581 repaired real Herdr
response decoding/absence checks; #582 repaired the demonstrated SQLite
Task-start deferred-to-writer upgrade contention. #583 verified commit
`6a7b5fd224d607c04026b089bb64a0b628e8b8a1` with these results:

| Check | Recorded result |
| --- | --- |
| CLI/server build; serial workspace tests | Passed; 123 tests plus doc/zero-test targets |
| Formatting; diff whitespace | Passed |
| Workspace Clippy | Completed with existing warnings |
| Strict Clippy (`-D warnings`) | Failed on 10 existing forge-app diagnostics |
| Default-parallel workspace tests | Failed on existing server ownership-lock `WouldBlock` flake; focused test subsequently passed 20/20 |
| Recovery race suite | 30/30 passes, including 1,920 fresh-store three-way resume/acknowledgement/reconciliation schedules |
| Controlled competing writer/start and bounded-timeout regressions | Each passed 50/50; no partial transition on timeout |

Automated RPC/CLI/persisted-store/provider-contract coverage includes redaction,
revocation and subscription termination on failed cleanup, confirmations and
immutable audit after reopen, retained known/pending blockers, identity mismatch,
surviving/malformed provider resources, late acknowledgement, separate resume,
and exactly one replacement under competing scans. Credential-field rejection
does not establish operator authentication.

Fresh isolated live verification used Herdr 0.9.5 protocol 22, Pi 1.5.0 and profile
model `gpt-5.6-terra`. Three attempts exercised real bootstrap, authenticated
acknowledgement/start/instruction retrieval, corrected nested `agent.get`, failed
cleanup after revocation, durable blockers across daemon restart, verified retry,
explicit unknown resolution and separate resume admitting one replacement.
Repeated scans/restart retained the same attempt, without bootstrap/prompt
replay. All owned agent/tab/pane sets and the isolated workspace were verified
removed; limited smoke children were checked unable to act before safety
assertions. Production Assignments/resources were not mutated. Unsafe live
identity/child-process fault injection was not performed; adversarial cases were
covered by automated contracts instead.

Evidence from #583 is retained locally in `/tmp/forge-583-checks` (checks/stress)
and `/tmp/forge-583-evg5943m` (isolated store, Git/worktrees, scripts and live
transcripts). These are temporary investigation artifacts, not packaged release
assets or a durable archive. Consult the Forge comment for exact commands and
outcomes; do not distribute capability files with a handoff.

This is **not an entirely green default-parallel/strict-lint release gate**. The
ownership-test flake and lint debt remain explicit limitations, distinct from the
fixed recovery contention. No universal elimination of SQLite failures is
claimed. No release publication, package/install verification, rollback rehearsal
or production migration/recovery check is reported by #583 or this documentation
handoff. A deployment owner must apply the release policy and decide whether
these limitations permit promotion; this document does not silently waive them.
