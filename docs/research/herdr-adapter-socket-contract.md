# Herdr V1 adapter socket contract

**Question:** What exact provider-neutral contract and Herdr Socket API sequence should Forge use for a V1 Task assignment?

**Decision:** implement one `HerdrAdapter` behind Forge's provider-neutral dispatch port. It provisions one dedicated **tab** and **pane** inside the registered Project's existing Herdr workspace; it never creates or closes that Project workspace. Every external call is made by the Forge daemon, not by the CLI.

## Sources and compatibility boundary

- [Herdr Socket API](https://herdr.dev/docs/socket-api/), retrieved 2026-10-22. It identifies the raw socket API as the appropriate layer for custom protocol clients and event subscribers, documents the methods below, and says that snapshot recovery must subscribe first and then reconcile from authoritative reads.
- Local installed protocol schema: `herdr api schema --json`, retrieved 2026-10-22: protocol **22**, schema version **1**. This is the version-specific source for parameter names, response shapes, states, subscriptions, and error envelopes. It must be checked into neither Forge data nor a profile; the adapter is compiled/tested against it.
- Persistence and reconciliation decisions: Design agent profile and assignment persistence (#436) and Design reconciliation lifecycle and concurrency (#437).

A configured socket is usable only when the daemon can connect and make `ping`, then obtain `session.snapshot`. The snapshot's reported protocol must be 22 (the adapter's supported protocol) and its response must validate against the installed schema. This is the V1 handshake/probe; protocol or schema mismatch is a non-retryable endpoint diagnostic, not an assignment failure. The Socket API documents `session.snapshot` as containing version/protocol metadata, but not as a subscription. Do not invent a separate authentication exchange: the V1 endpoint is a local Unix socket and protocol 22's public schema exposes no authentication handshake method.

On connection establishment/re-establishment, the long-lived observer sends `events.subscribe` for `pane.agent_status_changed`, then reads `session.snapshot` and refreshes the assignments it owns. A disconnect marks the observer stale and causes the next reconciliation to reconnect/probe/snapshot. Events merely accelerate a reconciliation request; they never mutate an assignment directly and never replace polling.

## Provider-neutral port

The dispatch application calls these operations. The port exposes Forge-owned assignment IDs and generic resource identities, never Herdr names outside this adapter.

```text
probe() -> EndpointHealth
provision(assignment, projectWorkspace, initialPrompt) -> ProvisionedAttempt
observe(assignment) -> Observation | ObservationTransportError
recover(assignment) -> RecoveryResult
retire(assignment) -> RetirementResult
subscribeStatusChanges(wake) -> ObserverHandle
```

- `EndpointHealth` distinguishes unavailable socket, incompatible protocol/schema, and healthy.
- `ProvisionedAttempt` returns durable `workspace`, `tab`, `pane`, and `agent-name` resources. The caller persists each returned resource immediately, as required by #436.
- `Observation` is one of `working`, `idle`, `blocked`, `done`, or `unknown`, with the observed `state_change_seq` and `completion_seq` when supplied. A missing or mismatched owned resource is reported as **absent**, not guessed as `done`.
- `recover` searches durable resources and proves either that they name the same owned attempt or that one is absent/conflicting. It does not create anything.
- All methods return a typed classification: `endpoint` (connect/protocol), `transient` (transport/read), `not-found`, `conflict`, `invalid-request`, or `cleanup`. The reconciliation rules in #437 decide closure/retry; in particular, transient observation errors consume no retry budget.

## Names, ownership, and discovery

For assignment ID `A`, use deterministic, collision-resistant labels/names derived solely from `A`:

```text
tab label:   forge-dispatch-A
agent name: forge-dispatch-A
```

The exact safe encoding is `forge-dispatch-` plus the assignment's opaque ASCII identifier; reject/encode any character Herdr does not accept rather than truncating into a collision. The adapter treats the assignment's persisted resource IDs as authority. Labels/names are recovery hints only: another pane with the same text is not adoptable.

`projectWorkspace` is found from the registered Project folder by `session.snapshot` / `workspace.list`: it must be a workspace whose `cwd` is the validated Project folder. Zero matches is a per-assignment provisioning failure; multiple matches is a conflict diagnostic. Forge must not use focus-dependent optional targets or create a replacement workspace. The selected workspace ID is persisted before later resources.

For recovery, read `agent.get` using the persisted agent name and require its returned `workspace_id`, `tab_id`, and `pane_id` to equal the persisted IDs. If name lookup is unavailable or ambiguous, use `agent.list` and match the complete persisted tuple. Read `tab.get` and `pane.get` where needed to classify a partial resource set. A resource found in another workspace/tab/pane is a conflict, not an owned agent.

## Exact provisioning mapping

The daemon owns a provision lease from #437; all steps are idempotent by recovery before create. It persists the response's identifier after **each** successful step before issuing the next one.

| Step | Socket method and parameters | Persist / recovery rule |
|---|---|---|
| Discover | `session.snapshot`, then `workspace.list`/`workspace.get` as needed | Find and persist the existing Project `workspace_id`; never call `workspace.create`. |
| Dedicated tab | `tab.create { workspace_id, label: "forge-dispatch-A", cwd: projectFolder, focus: false }` | Persist returned `tab_id`. Before create, recover only a tab ID already persisted for A; do not adopt a label-only tab. |
| Dedicated pane | `pane.split { target_pane_id: <an existing pane in the new tab>, workspace_id, direction: "right", focus: false }` | Persist returned `pane_id`. This makes Forge's pane dedicated rather than taking over the tab's root pane. The root pane remains part of the dedicated tab and is closed with that tab at retirement. |
| Start Pi | `agent.start { name: "forge-dispatch-A", kind: "pi", pane_id, args: <the immutable Task profile arguments>, timeout_ms: <profile startup timeout> }` | Persist `agent-name` (and re-read `agent.get` to confirm the returned identity tuple). `name`, `kind`, and `pane_id` are required by protocol 22. |
| Submit initial work | `agent.prompt { target: "forge-dispatch-A", text: initialPrompt }` | A successful response means prompt admission, not Task completion. No `wait` option is used by reconciliation. |

The profile defines `kind: "pi"`, model `gpt-5.6-terra`, prompt template, arguments, and timeout policy; the adapter must not add a mutable model or prompt override. `agent.prompt` returning `agentblocked` is not resubmitted: observe the retained assignment and follow #437's blocked-state policy. If a crash occurs between calls, `recover` resumes from the highest persisted resource; if a returned resource was not persisted, it must be discovered by the complete assignment identity or all known resources are retired and the attempt closes as `provision-failed`/`cleanup-abandoned` per #437.

## Observation and events

On every reconciliation, call `agent.get { target: <persisted agent-name> }`; on `not-found`, use `agent.list` once to distinguish absent from a name/resource conflict. Require the complete persisted workspace/tab/pane tuple on every successful read. Record the returned status and sequences conditionally against the still-open assignment.

Protocol-22 `AgentInfo` supplies `agent_status`, `launch_pending`, `interactive_ready`, `state_change_seq`, optional `completion_seq`, and resolved workspace/tab/pane IDs. Normalize only `agent_status` directly:

| Herdr | Adapter |
|---|---|
| `working` | `working` |
| `idle` | `idle` |
| `blocked` | `blocked` |
| `done` | `done` |
| `unknown` | `unknown` |

`launch_pending` and `interactive_ready` are supporting facts, not new Forge states. Do not infer Task completion from `done`; #437 owns that decision. An observed absent agent is `absent`, which #437 handles as loss when the Task is not done. `unknown` remains an observation until its durable grace period expires.

Subscribe with `events.subscribe { subscriptions: [{ type: "pane.agent_status_changed", pane_id }] }`. The event contains the pane/workspace identity and status; ignore events whose tuple does not match an open assignment. Queue a reconcile wake only. The documented Socket API explicitly requires a post-subscribe authoritative reconciliation and a new snapshot after reconnect/staleness, so delayed, duplicate, or missed events are harmless.

## Retirement and cleanup

Retirement is repeatable and records each outcome in `agent_assignment_resources`; it never deletes assignment history.

1. Re-observe/recover the durable tuple. If the pane still identifies `forge-dispatch-A`, call `pane.release_agent { pane_id, source: "forge-agent-dispatch", agent: "forge-dispatch-A", seq: <next adapter sequence> }`. A not-found release is already-clean evidence; a conflict is not.
2. Call `pane.close { pane_id }` for Forge's dedicated split pane. If already absent, record it retired. Never close a pane unless its persisted ID is in the assignment's dedicated tab and recovery confirms ownership.
3. Call `tab.close { tab_id }`, which removes the dedicated tab and its remaining root pane. Persist the tab/pane retirement facts.
4. Do **not** call `workspace.close`: the workspace is Project-owned, not assignment-owned.

Protocol 22 has no agent-delete/close API. `pane.release_agent`, `pane.close`, and `tab.close` are therefore the concrete release sequence. If cleanup is temporarily unavailable, leave resources and retry on a later reconciliation. If it is conclusively abandoned, retain all facts and use #437's `cleanup-abandoned` closure reason.

## Error mapping

Socket responses use an error envelope with string `code` and `message`; retain the code in redacted diagnostics. Map connection failure, malformed response, and incompatible snapshot/protocol to `endpoint`; timeouts/disconnects after a healthy probe to `transient`; missing persisted resources to `not-found`; wrong tuple/name or unexpected existing resource to `conflict`; validation failures and `agentblocked` to `invalid-request` (with `agentblocked` handled as a normal blocked observation where possible). A cleanup call failure is `cleanup` and does not erase resources or justify a duplicate provision.

This contract preserves #437's key properties: external calls occur outside the SQLite claim transaction; durable assignment/resource state makes them idempotent; authoritative reads—not event delivery—drive state; retained idle/blocked attempts consume capacity; and only Forge-created tab/pane resources are cleaned up.
