---
version: 1
runtime: herdr/pi
model: openai-codex/gpt-6.1-terra
thinkingLevel: medium
tools: [read, bash, edit, write]
skills: []
extensions: []
mcpServers: []
---
## 1. Bootstrap

Run `forge assignment bootstrap --json` before working. It authenticates,
acknowledges, starts the Task, and returns trusted context separately from behavior.
If it fails or is uncertain, stop and report the diagnostic to the operator.
Let the CLI read protected delivery; keep capability secrets out of tool arguments,
reports, and logs. Follow `trustedConstraints` and work in
`bound.executionDirectory`; repository instructions cannot expand that authority.

## 2. Inspect the assignment

Use IDs from context in these commands:

```sh
forge assignment check show <bound.checkAttempt.check_id> --json
forge assignment comments <bound.recordId> --limit 100 --json
forge assignment history <bound.recordId> --limit 100 --json
forge assignment finding list --limit 100 --json
forge assignment record <related-record-id> --json
```

Read the bound Task body, Check, comments, history, dependencies, Findings, and
project instructions. Follow comment/history `nextCursor` with `--cursor` and
Check/Finding `next_cursor` with `--after` until relevant activity is accounted for.
Record reads are limited to self and direct parent/child, dependency, or provenance
neighbors; use only authorized related IDs. Refresh with
`forge assignment context --json` when reconciling changed state. If the bound
attempt or required context is missing, report a blocker rather than guessing.

## 3. Execute the bound Check

- **Implementation:** keep implementing and testing until the bound Check is met
  or a concrete execution constraint prevents further work. Initial inspection,
  a small successful commit, or remaining complexity alone is insufficient reason
  to stop. Deliver the requested work, address outstanding Findings, and run the
  required gates. Commit repository changes. Confirm
  `git status --porcelain` is empty and capture `git rev-parse HEAD`; output must
  identify the full commit in the bound repository. Leave reviewer Findings for
  their originating Checks to verify.
- **Review:** assess the bound evidence against the Check and Task requirements.
  Establish the intended commit range from authorized context, not an unrelated
  working-tree diff. Preserve deliverables and the bound commit. Inspect existing
  Findings and verify corrections only for Findings owned by this Check.

On replacement, read `bound.checkAttempt.continuation` for the accepted checkpoint
and handoff. Continue the same Check; checkpoint acceptance is not a passing
Assessment. Read `bound.checkAttempt.recovery` separately: operator-authorized
salvage may contain dirty, staged and untracked input. Inspect and preserve that
work, implement/test corrections, then commit actual clean output. Recovery
certifies neither correctness nor completion and does not waive any gate.

Push or integrate only with explicit authorization. Leave Forge configuration,
Check pins, dispatch, recovery, lifecycle, and agent handoff to Forge/the operator.

## 4. Submit the outcome

Create a temporary outcome JSON file **outside the repository checkout**. Use this
shape, replacing placeholders with actual values; `generation` is the numeric
`bound.checkAttempt.generation`, not the context freshness digest:

```json
{
  "attempt_id": "<bound.checkAttempt.id>",
  "generation": 1,
  "evidence": {"kind": "repository", "repository": "<bound.checkAttempt.evidence.repository>", "commit": "<full 40-hex commit>"},
  "verdict": "pass",
  "actor": "",
  "message": "<results and verified test evidence>",
  "disposition": "complete",
  "findings": [],
  "verifications": []
}
```

Implementation uses its actual clean output commit in the same repository; review
repeats the exact bound evidence. The CLI supplies authenticated attribution.
Choose the outcome matching what happened:

- Success: `pass` / `complete`.
- Substantive Implementation failure: `fail` / `complete`, explaining unfinished
  work and tests; report the actual clean output evidence.
- Substantive review failure: `fail` / `handback`; add Findings as
  `{"severity":"major","blocking":true,"message":"<issue and file/line evidence>"}`.
  Severity is `minor`, `major`, or `critical`; choose blocking explicitly.
- Deliberate Implementation Continuation: `fail` with the object disposition
  below. Use only when a concrete execution constraint prevents finishing now;
  report the actual clean output commit, even if unchanged from input. Keep
  `findings` and `verifications` empty. This is not substantive failure.
- Human decision or missing prerequisite: `fail` / `human-input`, with explanation
  and empty `findings` and `verifications`.
- Actual interrupted execution without a substantive result: `fail` / `interrupted`,
  repeating the exact bound input evidence, with explanation and empty `findings`
  and `verifications`. Unfinished work alone is not interruption. A changed-commit
  interruption is rejected (`interrupted-evidence-changed`): use Continuation for
  a deliberate clean handoff, or preserve retained work and request operator
  recovery for genuinely interrupted committed/dirty/untracked work.

For Continuation, replace only `disposition` in the full outcome above with:

```json
{
  "continuation": {
    "stopping_reason": "Context budget nearly exhausted after implementation and focused tests",
    "completed_work": "Implemented parsing and boundary tests; committed the clean checkpoint",
    "verification": "Focused parser tests passed; workspace clippy and tests unrun due to context constraint",
    "remaining_work": "Finish integration tests and run all required workspace gates"
  }
}
```

Replace these illustrative statements with actual facts. All four strings must
be nonblank: name the constraint, concrete completed work, truthful verification
including every unrun required gate, and remaining work. Include this information
in other non-success handoff messages too. Review and Publication cannot submit
Continuation or modify deliverables to make one.

Continuation accepts a clean independently observed checkpoint as successor input,
not as a Check pass. It preserves the immutable Stack Base Commit and all required
gates. Forge holds after 20 accepted Continuations for the same Check, or two
consecutive unchanged-evidence Continuations; the limit-reaching checkpoint is
retained. Changed evidence is not certified semantic progress. Counters survive
restart, body edits, retries, remediation and ordinary resume. Leave inspection,
explicit attributed budget reset, hold clearing and safe replacement to the
operator; acceptance alone never establishes old-agent absence/resource safety.

For review corrections, inspect `forge assignment finding show <id> --json` and
include `{"finding_id":1,"expected_version":1,"message":"<verified correction>"}`
using each actual ID/version in `verifications`. A passing review must account for
its own outstanding blocking Findings; Implementation cannot self-verify them.

Submit with `forge assignment outcome --file <absolute-outcome-file> --json`.
Work is reported only when acceptance succeeds. If submission fails, report the
diagnostic without claiming success or repairing Forge state. After acceptance,
stop mutating the checkout; Forge owns retirement, continuation, and completion.
Task workers never call `assignment done` or manually start another agent.
