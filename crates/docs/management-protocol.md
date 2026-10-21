# Forge Management Protocol revision 1

This document is the Forge-side handoff for Forgeup consumers. It defines the
released Management Protocol surface only; it does not implement Forgeup
registry, installation, activation, selector resolution, or managed-CLI
behavior.

## Revision Identity and offline inspection

Both executables expose an offline, machine-readable Revision Identity:

```sh
forge --build-info --json
forge-server --build-info --json
```

Each command writes the same JSON object and neither contacts nor starts a
daemon:

```json
{
  "commit": "<full Git commit>",
  "dirty": false,
  "management_protocol_revision": 1,
  "version": "<build version>"
}
```

`commit`, `dirty`, `management_protocol_revision`, and `version` are immutable
facts compiled into the executable. Forgeup must inspect both candidate
executables and accept a pair only when their complete Revision Identities
match and revision `1` is supported. A Source Label belongs to the Forgeup
registry and is not part of Revision Identity.

## Daemon status and ownership

`system.status` retains its existing `name`, `version`, and `ready` fields and
adds `management`:

```json
{
  "management_protocol_revision": 1,
  "revision_identity": {
    "commit": "<full Git commit>",
    "dirty": false,
    "management_protocol_revision": 1,
    "version": "<build version>"
  },
  "daemon_instance_id": "<Forgeup-generated random ID or null>"
}
```

A consumer must explicitly reject an unsupported protocol revision; it must not
infer compatibility from package version or individual fields. Forgeup supplies
a fresh random daemon-instance ID for each managed launch, then requires that
exact ID and the expected Revision Identity in status before treating the
process as owned. A manually launched daemon reports `daemon_instance_id` as
`null` and remains usable through an explicit endpoint override.

## Managed launch

Forgeup starts a managed daemon with all of these explicit inputs:

```text
forge-server --managed \
  --bind-addr IP:PORT \
  --store-path PATH \
  --runtime-record-path PATH \
  --lifecycle-control-endpoint PATH \
  --lifecycle-credential VALUE \
  --daemon-instance-id ID
```

All managed inputs are required, non-blank, and take precedence over ambient
settings. Missing, malformed, or unbindable managed configuration is a startup
failure; Forge must not silently run it as a manual daemon. `FORGE_BIND_ADDR`
and `FORGE_STORE_PATH` remain manual-launch defaults only.

`runtime-record-path` is an explicit managed-launch input reserved for the
manager's per-instance runtime record. The lifecycle credential is secret and
must not be logged or exposed through status.

## Graceful lifecycle control

Managed lifecycle control is not an HTTP or JSON-RPC method. It uses the
per-instance endpoint supplied at launch:

- Unix: a Unix-domain socket.
- Windows: a named pipe.

To request shutdown, Forgeup connects locally and writes the lifecycle
credential followed by a newline. Forge accepts only an exact credential;
missing, malformed, oversized, or incorrect requests are rejected and leave
the daemon running. A valid request triggers graceful shutdown. Unix socket
cleanup occurs when the lifecycle listener exits. Ctrl-C and SIGTERM remain
recovery shutdown paths.

Forgeup should create endpoints with local-only access and use a fresh,
unpredictable credential per launch. The daemon's public HTTP/JSON-RPC listener
remains separate and unauthenticated; lifecycle authority is not granted by
knowing its endpoint.

## Compatibility boundary

Revision 1 covers offline Revision Identity, additive `system.status`
management facts, explicit managed launch, managed-daemon ownership
correlation, and authenticated local graceful shutdown. Compatible additions
retain revision 1; incompatible changes require a new monotonic revision.

This handoff deliberately excludes Forgeup registry entries and Source Labels,
installation and activation, executable selection, state retention, logs,
locking, downloads, release discovery, signatures, self-update, and ordinary
managed-CLI orchestration.
