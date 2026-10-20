# ADR-001: Daemon-First CLI and Web Interfaces

- **Status:** Accepted
- **Date:** 2026-10-19
- **Decision scope:** Application interfaces, communication, and technology selection

## Context

The application begins with a Rust command-line interface, but it should also support a browser-based interface. Both interfaces must operate against the same running application. In the intended deployment, a **single central daemon** serves clients on its host and on other computers connected through Tailscale.

The architecture should keep the CLI lightweight, avoid duplicating execution paths, and enable type-safe communication between the Rust CLI and daemon. The browser interface should use server-rendered HTML with minimal client-side JavaScript and no application-specific JavaScript/TypeScript frontend.

This ADR does **not** establish domain models, business use cases, or a concrete persistence/storage implementation.

## Decision

Adopt a **daemon-first client–server architecture** with two interfaces:

1. **CLI → daemon:** The Rust CLI always communicates with the central daemon over HTTP using a typed JSON-RPC contract, including when the CLI runs on the daemon's host.
2. **Browser → daemon:** The daemon serves HTML pages and fragments. HTMX enhances browser interactions by requesting server-rendered HTML over HTTP.

Both transport adapters delegate to a shared application boundary. Networking and rendering concerns stay outside that boundary. Internal calls between the web handlers and the application **do not** loop back through the RPC endpoint.

```text
   Local host or remote PC (via Tailscale)
   ┌──────────────────┐    ┌──────────────────┐
   │ Rust CLI         │    │ Browser          │
   │ clap             │    │ HTML + HTMX      │
   └────────┬─────────┘    └────────┬─────────┘
            │ JSON-RPC/HTTP         │ HTTP/HTML
            ▼                       ▼
   ┌──────────────────────────────────────────┐
   │ Central Rust daemon                      │
   │                                          │
   │ jsonrpsee RPC adapter   Axum web routes  │
   │          │                   │           │
   │          └─────────┬─────────┘           │
   │                    ▼                     │
   │          Shared application boundary     │
   │                    ▲                     │
   │                    │ application-owned   │
   │                    │ persistence needs   │
   │          Storage adapter (`store`)       │
   └──────────────────────────────────────────┘
```

## Technology choices

| Package | Responsibility |
| --- | --- |
| **clap** | CLI argument parsing, subcommands, and daemon endpoint configuration |
| **axum** | Daemon HTTP serving, web routes, and HTML/form request handling |
| **jsonrpsee** | JSON-RPC contract and typed Rust client/server support |
| **askama** | Compile-time checked server-side HTML templates |
| **htmx** | Browser interactions through HTTP requests and HTML fragment swaps |

The browser does not require a custom JavaScript/TypeScript application. HTMX itself is a small JavaScript library delivered to the browser.

## Interface and module boundaries

The current workspace layout is:

```text
crates/
  cli/       # clap commands and RPC client
  server/    # daemon startup, RPC implementation, HTTP/HTML routes
  rpc/       # shared wire-level contracts and DTOs
  app/       # shared application boundary (internals undecided)
  store/     # storage adapter for application-owned persistence needs
```

This layout records the current ownership boundaries; future crate boundaries may evolve.

This layout illustrates ownership, not a permanent requirement for exactly five crates. In particular, this ADR makes no decision about domain modules or concrete storage technology.

- **CLI:** Translates command-line arguments into RPC requests, displays responses/errors, and supports configuring the daemon URL. It does not bypass the daemon to execute application operations.
- **RPC contract:** Defines serialized request/response types, methods, and transport-level errors. The CLI and daemon share these definitions; the contract should not require the CLI to depend on daemon implementation code.
- **Daemon:** Hosts the RPC and HTML interfaces, owns the running application instance, composes the storage adapter, and delegates requests to the shared application boundary.
- **Storage adapter:** Implements persistence needs owned by the application from the adapter side. It depends on the application boundary; the application does not depend on a concrete storage implementation.
- **Web interface:** Uses Axum routes and Askama templates. HTMX endpoints return HTML fragments as appropriate. Web handlers call the application boundary directly rather than making HTTP requests to the daemon's own RPC API.

## HTTP serving approach

Expose a logical RPC interface (for example, `/rpc`) and regular web routes (for example, `/` and `/items`). The actual paths are illustrative and are not fixed by this ADR.

Axum and jsonrpsee must be integrated deliberately: sharing a Tokio runtime does not automatically mean they share the same listening port. The implementation may mount a compatible RPC service into the HTTP router or use distinct listeners if integration proves unnecessarily complex. The externally visible interfaces remain the same.

## Deployment and access

- **One authoritative daemon:** A single machine hosts the application; other computers act as clients rather than independent daemon replicas.
- **Local access:** The host's CLI and browser connect to the same daemon interfaces as remote clients.
- **Remote access:** Clients may reach the daemon through Tailscale. A configurable endpoint allows the CLI to switch between local and remote addresses.
- **Security:** Tailscale provides private network connectivity but does not replace authorization for sensitive operations. HTTP exposure, authentication/authorization, browser origin checks, and CSRF protections must be addressed before enabling remote or browser-based mutations.
- **Lifecycle:** The daemon runs independently of CLI processes. The specific service-management mechanism is a deployment decision outside this ADR.

## Consequences

### Benefits

- Consistent CLI behavior locally and over Tailscale.
- A single running application instance for CLI and browser users.
- Type-safe Rust RPC client/server definitions reduce contract drift.
- Server-rendered UI avoids maintaining a separate SPA and frontend build stack.
- Transport-specific handlers can evolve without embedding networking details in application operations.

### Trade-offs

- The CLI depends on daemon availability, even for local commands.
- RPC and HTML expose different response formats and require separate adapters.
- RPC contract compatibility must be managed as CLI and daemon versions diverge.
- The central daemon is a single point of availability; Tailscale supplies networking, not replication or offline synchronization.
- HTMX requires browser JavaScript, even though no custom JS/TS frontend is planned.

## Explicitly deferred

- Domain entities, business rules, and concrete application operations.
- Concrete storage technology, persistence layout, and synchronization strategy.
- Authentication/authorization mechanism and detailed network exposure configuration.
- Final route names, template organization, service manager, and deployment packaging.

## Decision summary

Use **clap** for a Rust CLI that **always calls a central daemon via jsonrpsee JSON-RPC over HTTP**. Use **axum + askama + htmx** to provide server-rendered browser functionality from that same daemon. Both interfaces invoke a shared application boundary. The daemon composes a storage adapter that depends on that boundary, without selecting a concrete storage technology or persistence design.
