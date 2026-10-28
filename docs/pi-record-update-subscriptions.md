# Pi Record Update Subscription Extension

> **Status:** Implemented V1 integration guide.

The Forge project extension lets a Pi process launched for an open Agent Assignment manage durable Record Update Subscriptions and receive their SSE wake notifications. It does **not** read authoritative Record state: a wake is a prompt to reconcile through the Assignment's authorized Forge read capability.

The wire-level lifecycle, SSE framing, authorization, cursor validation, and recovery rules are separately normative in the [Record Update Subscription SSE replay and recovery contract](research/record-update-subscription-sse-contract.md). This guide describes the Pi launch and consumer boundary.

## Prerequisite: Forge-launched Pi Assignment

Configure Forge's dispatch provider and reconcile it as described in the [dispatch operator guide](../README.md#operators-and-recovery). Forge launches Pi in the Assignment's Agent Workspace and supplies this process environment:

- `FORGE_ASSIGNMENT_ID`
- `FORGE_ASSIGNMENT_DAEMON_URL` — daemon root URL; the extension derives JSON-RPC and SSE URLs.
- `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE` — absolute path to an owner-only Assignment Capability file outside the Agent Workspace.

The capability value is never in the prompt or an environment variable. The extension reads its file immediately before each management request, never caches, displays, or writes its value. If configuration is absent or invalid, no model-callable subscription tools are registered. `/forge-subscriptions` reports only safe availability diagnostics.

## Agent workflow

The configured session exposes four model-callable tools:

1. `forge_subscribe` creates or returns the idempotent subscription for explicit target Record IDs and optional update-kind filters. Record `subscription_id`, `start_cursor`, and lease expiry from its result. Its `reconcile_required: true` result is mandatory: read the selected Records through authorized Forge reads before treating a later notification as useful.
2. `forge_wait_for_updates(subscription_id, after, max_events?, timeout_ms?)` performs one bounded SSE wait. Pass an explicit durable cursor in `after`; the defaults are 50 envelopes and 30 seconds, with maxima of 256 envelopes and five minutes. The extension retains no cursor, reconnect, or renewal state.
3. For every non-empty `updates` result, reconcile the affected Records authoritatively, then wait again with `next_cursor`. SSE envelopes are immutable notifications, not Record state, and may be duplicated across reconnects.
4. `forge_renew_subscription` explicitly extends the 30-minute lease when needed. `forge_delete_subscription` explicitly removes it when wake delivery is no longer needed.

Subscriptions survive Pi reload and shutdown until their lease expires, their owner retires, or the Agent deletes them. Only one local wait may be active per subscription; this avoids replacing an earlier wait under Forge's last-connected-wins delivery rule.

## Recovery outcomes

`forge_wait_for_updates` returns typed normal outcomes rather than silently recovering:

- `updates` — reconcile the affected Records and continue from `next_cursor`.
- `timeout` — no update arrived during this bounded wait; retry deliberately from `next_cursor`.
- `connection_lost` — reconcile first, then open a new wait from the returned cursor; the extension never reconnects itself.
- `resync_required` — reconcile first; use its returned cursor facts, including `head`, to choose the next explicit cursor.
- `subscription_expired` or `subscription_deleted` — stop waiting; create a new subscription only when the Assignment still needs one.

Management tools similarly return expected outcomes such as `not_configured`, `authorization_denied`, `subscription_expired`, and `subscription_deleted`. Malformed daemon responses and unexpected transport failures are tool errors, not recoverable notification results.

## Release handoff

### Release note

Forge now includes a Pi project extension for assignment-owned Record Update Subscription wakeups. It is available only to Pi sessions launched by Forge for an open Agent Assignment; it is not a general-purpose remote notification channel. Wakes are at-least-once hints, so agents must reconcile through authorized Forge reads before acting on them.

### Operator preflight

1. Configure and reconcile the Forge dispatch provider as described in the [dispatch operator guide](../README.md#operators-and-recovery). Do not manually set the extension's launch variables or copy a capability value into a prompt, shell environment, or workspace.
2. Confirm the launched Pi process has all three launch variables listed above and that the secret-file path is absolute, outside its Agent Workspace, and owner-only. Forge creates, owns, and cleans up that file.
3. Confirm `/forge-subscriptions` reports configured availability without revealing a capability. Absence of model-callable tools outside this launch context is expected.
4. Ensure consumers retain an explicit cursor, use one bounded wait at a time per subscription, renew a lease deliberately, and reconcile after every wake or recovery outcome.

### Support and recovery

- `not_configured` means the session was not launched with a valid Assignment Capability Delivery contract; correct dispatch configuration and relaunch rather than supplying credentials manually.
- `authorization_denied`, `subscription_expired`, and `subscription_deleted` are lifecycle outcomes. Recreate a subscription only while the Assignment still needs it and remains authorized.
- On `connection_lost` or `resync_required`, reconcile authoritative Record state before opening another wait from the returned cursor facts. Do not treat an SSE envelope as current Record state.
- Malformed daemon responses and unexpected transport failures are errors to investigate at the daemon/extension boundary; they are not notification recovery outcomes.

### Verified release evidence

The release gate passed `cargo fmt --check`, `cargo test --workspace`, `git diff --check`, and the local extension harness:

```sh
NODE_PATH=/home/a/.volta/tools/image/packages/@earendil-works/pi-coding-agent/lib/node_modules/@earendil-works/pi-coding-agent/node_modules \
  node --test tests/forge-subscriptions.test.mjs
```

That harness covers launch configuration, tool gating, capability rotation and non-disclosure, JSON-RPC lifecycle management, bounded SSE waits and recovery, and assignment launch through committed-update wakeup. Workspace Clippy with warnings denied remains blocked by five pre-existing, unrelated `forge-app` lint violations (four missing `# Errors` docs and one `match_same_arms`); no integration defect was found.
