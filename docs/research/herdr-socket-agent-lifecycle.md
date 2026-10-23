# Herdr socket lifecycle findings

**Question:** Which Herdr Socket API capabilities and states can a Forge adapter rely on to provision, observe, and clean up a Pi agent?

## Sources

- [Herdr Socket API](https://herdr.dev/docs/socket-api/), retrieved 2026-04-26.
- Local Herdr protocol schema: `herdr api schema --json`, retrieved 2026-04-26 (protocol 22, schema version 1).

## Findings

1. Herdr explicitly positions the raw Socket API for custom tools, protocol clients, and event subscribers. It exposes agent start, inspection, prompt, wait, and event subscription, so it is an appropriate boundary behind Forge's provider-neutral adapter. [Socket API](https://herdr.dev/docs/socket-api/)
2. The current installed schema defines the adapter-relevant agent states as `idle`, `working`, `blocked`, `done`, and `unknown`. `AgentInfo` includes the resolved pane, workspace, and tab identifiers, `agent_status`, `launch_pending`, `interactive_ready`, `state_change_seq`, and optional `completion_seq`.
3. Agent creation is a two-stage operation in protocol 22: `agent.start` requires `name`, `kind`, and `pane_id` (with optional argv and startup timeout); `agent.prompt` takes a target and text. Prompt may include an atomic wait request, avoiding a separate prompt/wait race. The Socket API documentation cautions that a prompt to an already blocked agent returns `agentblocked` without submitting the prompt.
4. `agent.get` and `agent.list` are the polling/recovery reads. `session.snapshot` is the authoritative bootstrap/recovery read for a client-side cache; the documentation says to subscribe first, then reconcile from authoritative reads, and to obtain a new snapshot after reconnect or suspected staleness.
5. A long-lived adapter can subscribe to `pane_agent_status_changed`; the event carries the pane and workspace identifiers plus the agent status. It must still reconcile after reconnect because events cannot unconditionally replay over a snapshot. [Socket API](https://herdr.dev/docs/socket-api/)
6. The schema exposes `agent.wait`, which is server-owned and pins the resolved pane occupant so a replacement cannot satisfy a previous wait. This is useful for a short, request-scoped wait but does not replace Forge's durable reconciliation state.
7. There is no `agent.close` method in the protocol-22 method set. The closest lifecycle operations are `pane.release_agent` (releases agent authority from a pane) and `pane.close`/`workspace.close`. Therefore the adapter design must explicitly determine whether V1 cleanup releases the agent, closes its dedicated pane/workspace, or both; it must not assume an agent-delete operation exists.
8. The installed schema and docs use both agent names and pane targets. A Forge Agent Assignment should persist the stable Herdr resource identifiers needed to re-read the attempt—at minimum the pane ID and the created agent name, and likely workspace/tab IDs if Forge owns their lifecycle.

## Adapter-facing recommendation

Expose provider-neutral operations rather than Herdr terms: provision an agent in a supplied workspace, submit its initial prompt, observe a durable attempt, and retire its owned resources. Implement observation by authoritative `agent.get`/`agent.list` reads on every reconciliation; event subscriptions are an optimization, not a source of truth. Map Herdr `working` to active work, `idle` to the user-specified idle cases, and preserve `blocked`, `done`, and `unknown` distinctly until reconciliation design defines their policy.

Before implementation, verify the exact Herdr workspace/pane provisioning sequence and select an explicit cleanup policy for the dedicated resources. The public page establishes available controls but does not prescribe Forge's ownership model.

## Research limitation

A follow-up site search could not run because the configured Ketch Brave backend lacks an API key. No configuration was changed. The findings above are based on the official Socket API page and the locally installed Herdr protocol schema.
