# Implementation Continuation profile delivery (T06)

## Source and review

[implementation-continuations.md](implementation-continuations.md) is a complete
profile source delivery package, **not an effective registered profile**. Origin:
Factory Workspace `/home/a/projects/factory/.agents/profiles/implementation.md`,
read-only input SHA-256
`00545ecf96ee56d63ee1f8bbb2288d858d475b8080b45e98c61f02be60ff4648`.
The external source was not overwritten. The repository artifact preserves its
frontmatter byte-for-byte: runtime `herdr/pi`, model
`openai-codex/gpt-6.1-terra`, thinking `medium`, tools read/bash/edit/write, no
skills/extensions/MCP. Bootstrap, context reads, reviewer authority and lifecycle
rules remain intact. Only stopping, outcome and successor guidance changes.

The tracked [origin snapshot](implementation-continuations-origin.md) makes the
source diff reviewable without access to the Workspace:

```sh
git diff --no-index docs/profiles/implementation-continuations-origin.md docs/profiles/implementation-continuations.md
```

Exit status 1 means the expected source differences. The snapshot is provenance,
not an alternative rollout candidate.

## Isolated validation

The `implementation_continuations` application tests copy the delivery into a fresh
fixture Workspace's `.agents/profiles/`, resolve it with the real catalog, compare
its resolved policy with the origin, validate its JSON examples and reject malformed
or role-inappropriate Continuations. CLI documentation tests parse commands without
execution. No live profile or provider is used.

Focused review checklist (also guarded by profile text assertions):

- Continue implementing/testing to completion or a concrete constraint; inspection,
  a small commit and remaining complexity alone are insufficient stopping reasons.
- Distinguish success, substantive failure, deliberate Continuation, actual
  interruption and human input. Report actual results and unrun gates.
- Accepted clean checkpoints feed the same Check, not review/publication; dirty
  salvage is separate operator authority and must become actual clean output.
- Limit-reaching checkpoints remain retained; evidence changes are not semantic
  progress. Retirement, resources, holds and ordinary readiness still gate launch.

## Separately approved rollout — not executed by this delivery

1. Approve revision installation/activation under the release policy. Local tests
   do not establish release readiness. Use matched CLI/server versions: the object
   Continuation disposition and added context fields require supporting peers.
2. Approve explicit offline Revision State migration if needed. Current schema is
   **19**; ordinary opening rejects incompatible stores without writes. Schema-18
   v1 dirty recovery authorizations are preserved but fail closed under corrected
   fingerprinting; there is no automatic in-place reauthorization. Preserve such
   evidence for separately approved recovery design, not storage edits. Follow
   [state upgrades](../state-upgrades.md), with isolated rehearsal/backups. Never
   infer historical Continuations, fingerprints or reset budgets during upgrade.
3. Approve copying the delivered candidate to the selected Workspace's
   `.agents/profiles/` authoring boundary, previewing, then registering/selecting
   it. Repository inheritance alone transfers no external source. Registration
   creates an immutable revision and selects it; it is a live mutation.
4. Inspect actual immutable Check pins. Workspace selection/template edits affect
   future Tasks, not existing Checks or retries. There is no supported existing
   Check repin operation. Restore pinned prerequisites or use supported Manual
   handling only with separate approval; never edit persistence to repin.
5. Approve eventual #37 recovery independently, using fresh public inspection and
   the [recovery runbook](../assignment-recovery.md). Historical incident commits
   and source delivery are not current ownership or salvage authorization. Verify
   old-agent absence and resource safety, authorize retained work if eligible,
   inspect budgets/holds, and separately resume/dispatch only when justified.

Budget inspection/reset and salvage command examples are authoritative in
[dispatch](../dispatch.md#deliberate-implementation-continuations) and
[recovery](../assignment-recovery.md#authorize-retained-implementation-work-for-salvage).
Reset clears only total/consecutive-unchanged accounting and appends attribution;
it preserves checkpoint/history, holds and ownership. Ordinary resume does not
renew exhausted allowance. No reset, checkpoint acceptance or salvage authorization
alone grants safe replacement or passes a Check.
