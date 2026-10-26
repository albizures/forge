# Pi Record Update Subscription Extension

> **Status:** Planned V1 integration. This document records the agreed extension behavior; it does not claim the extension or its Forge capability-delivery prerequisite is implemented.

The Forge project extension is available only to a Pi process launched for an open Agent Assignment. It provides durable Record Update Subscription management and SSE wake delivery; it does not provide authoritative Record reads.

## Assignment configuration

Forge dispatch supplies these session environment variables:

- `FORGE_ASSIGNMENT_ID`
- `FORGE_ASSIGNMENT_DAEMON_URL` — the daemon root URL; the extension derives its JSON-RPC and SSE routes.
- `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE` — an absolute owner-only file containing the Assignment Capability.

The extension reads the secret file immediately before a management request and never caches, displays, or writes the capability value. If configuration is absent or invalid, it hides the model-callable subscription tools. The `/forge-subscriptions` command reports safe configuration diagnostics without exposing credentials.

## Tools and workflow

The extension exposes four model-callable tools to configured Assignment sessions:

1. `forge_subscribe` creates or returns the idempotent subscription for explicit target Record IDs and optional update-kind filters. Its result contains the subscription ID, captured `start_cursor`, lease expiry, and `reconcile_required: true`.
2. The Agent reads the selected Records through an authorized Forge read capability. SSE notifications are not Record state.
3. `forge_wait_for_updates(subscription_id, after, max_events?, timeout_ms?)` connects with `Last-Event-ID: after`. It returns every received envelope in global Record Update ID order and a `next_cursor`. Defaults are 50 events and 30 seconds; requests are capped at 256 events and five minutes.
4. After a non-empty batch, the Agent reconciles the affected Records and waits again with `next_cursor`.
5. `forge_renew_subscription` explicitly extends the 30-minute subscription lease when needed. `forge_delete_subscription` explicitly removes it.

The extension maintains no hidden cursor or renewal state. Subscriptions survive Pi reload and shutdown until their lease expires, their owner retires, or the Agent deletes them.

## Recovery and concurrency

Only one wait may be active locally for a subscription. This prevents Forge's last-connected-wins SSE rule from replacing an earlier sibling call.

Expected wait results are typed: `updates`, `timeout`, `connection_lost`, `resync_required`, `subscription_expired`, and `subscription_deleted`. A connection loss never triggers a hidden reconnect: the Agent reconciles first, then opens a new wait. A `resync_required` result carries Forge cursor facts, including `head`; after reconciliation, the Agent resumes from the applicable returned cursor.

Management tools likewise return typed expected outcomes such as `not_configured`, `authorization_denied`, `subscription_expired`, and `subscription_deleted`. Malformed responses and unexpected transport failures are tool errors.

## Verification

V1 acceptance requires Forge tests for capability-file creation and Herdr environment delivery, extension tests against a local fake RPC/SSE server, and an end-to-end local bootstrap-to-SSE-wake test.
