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

## Verification

The V1 coverage includes:

- Forge tests for owner-only capability-file creation and Herdr's exact Pi environment delivery;
- extension tests against a local JSON-RPC/SSE server for management, cursors, bounds, recovery, and capability non-disclosure; and
- a local end-to-end harness that boots the extension from Assignment Capability Delivery configuration, creates a subscription for Record `41`, then receives its matching committed Record Update as an SSE wake.
