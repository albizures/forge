# Forge Runtime

The Forge Runtime context is the product domain implemented by Forge's Rust components. It exists to provide Forge's user-facing and supporting capabilities as one cohesive system.

## Language

**Forge Revision**:
An installed Forge build identified primarily by its source commit ID and optionally labelled with its release version.
_Avoid_: version, binary

**Active Forge Revision**:
The one user-wide selected Forge Revision that Forgeup has selected for use.
_Avoid_: current version, default version

**Forgeup**:
The independent `forgeup` program that registers Forge Revisions and selects the Active Forge Revision; it is not a Forge command-line interface.
_Avoid_: forgevm, forge launcher, forge CLI

**Revision Installation**:
An immutable Forgeup-owned snapshot and manifest for one Forge Revision that locates its CLI and daemon executables, endpoint, and isolated state.
_Avoid_: binary path, installation

**Source Label**:
An explicit immutable name that identifies the source of Forge Revisions and distinguishes independently built forks; its URL is descriptive metadata only.
_Avoid_: remote URL, source identity

**Revision Selector**:
A human reference to a Forge Revision, canonically its Source Label and full commit ID; a local-build label may be a unique alias.
_Avoid_: version, release version

**Revision State**:
The isolated persistent database, runtime information, and diagnostic logs associated with one Forge Revision. It outlives that Revision’s Installation unless explicitly purged.
_Avoid_: installation, global state

**Replacement Registration**:
An explicit Forgeup operation that replaces one Revision Installation under the same Revision Selector while retaining its Revision State unless the user requests a purge.
_Avoid_: update, ordinary registration

**Managed Daemon**:
The daemon process Forgeup owns for the Active Forge Revision. Manually launched daemons are not Managed Daemons.
_Avoid_: Forge daemon, server

**Manual Daemon**:
A Forge daemon launched outside Forgeup’s lifecycle and addressed only through an explicit endpoint override.
_Avoid_: alternate daemon, unmanaged server

**Endpoint Override**:
An explicit Forge CLI endpoint selection that bypasses the Active Forge Revision and its Managed Daemon.
_Avoid_: daemon selection, server configuration

**Revision Identity**:
The immutable build facts that attest a Forge Revision: full commit ID, clean or dirty state, Management Protocol, and build metadata. A Source Label is Forgeup registry metadata, not a build fact.
_Avoid_: source label, version, package version

**Management Protocol**:
The versioned Forge interface that lets Forgeup attest and control a Managed Daemon. It uses monotonic integer revisions: compatible additions retain a revision; incompatible changes introduce a new one. Revisions without a supported Management Protocol may only run manually.
_Avoid_: daemon API, server protocol

**Daemon Instance**:
One launch of a Managed Daemon, identified by a Forgeup-generated random instance ID that the daemon reports for ownership verification.
_Avoid_: revision, daemon version

**Project**:
A Forge-owned unit that is bound to exactly one Repository, has one Registered Folder that is a main Worktree, and to which Specs will belong. A Project is referenced by its immutable Slug.
_Avoid_: repo

**Repository**:
A local Git repository identified by its checkout, not by its remote address. Independent clones are distinct Repositories even when they share a remote; registration requires the Repository to have at least one configured Remote URL, but Repository identity has no cross-Project behavior currently.
_Avoid_: project, remote repository

**Worktree**:
A checked-out folder of a Repository. Only a main Worktree may be registered as a Project; a Repository's other Worktrees are workspaces of that Project rather than separate Projects.
_Avoid_: project folder

**Managed Worktree**:
A Worktree created and durably tracked by Forge for an eligible Record under its Project.
_Avoid_: task checkout, Forge workspace

**Worktree Claim**:
Forge's exclusive durable association of one eligible Record with one Managed Worktree. The claim prevents a second Managed Worktree for that Record.
_Avoid_: worktree assignment, checkout lock

**Worktree Root**:
The user-configured folder beneath which Forge places Managed Worktrees by Project Slug and Record Identifier.
_Avoid_: worktree base, checkout directory

**Project Setup Hook**:
An optional Project-configured shell command run in a newly created Managed Worktree.
_Avoid_: install command, bootstrap script

**Worktree Safety Assessment**:
Forge's local, offline evaluation of whether a Managed Worktree can be ordinarily removed and the facts supporting that result.
_Avoid_: cleanup check, removal status

**Registered Folder**:
The main Worktree folder associated with a Project during registration. Registered Folders cannot be equal to, ancestors of, or descendants of another Project's Registered Folder.
_Avoid_: project folder

**Remote URL**:
A Git remote endpoint configured for a local Repository. Registration requires a remote named `origin` and records its URL.
_Avoid_: repository identity

**Slug**:
An immutable, human-referenceable identifier for a Project. No two Projects share a Slug.
_Avoid_: project name

**Record**:
Forge’s generic primary entity for representing work across multiple workflows. Its common capabilities form the foundation from which more specific Record types, such as Specs, Tasks, and Epics, impose additional constraints.
_Avoid_: v1 entity

**Record Type**:
The immutable kind selected when a Record opens. Generic Record, Spec, and Task are the initial Record Types.
_Avoid_: category, label

**Record Identifier**:
An opaque positive numeric identity that uniquely references a Record across Forge interfaces.
_Avoid_: record slug, record name

**Spec**:
A Record type that defines planned work for exactly one Project and may parent Tasks.
_Avoid_: specification, issue

**Task**:
A Record type that represents a unit of work with zero or one Project Scope. It may be opened under one Spec, from which it receives its Project Scope, or opened without a parent; its membership and scope are immutable after opening, and it may not parent another Record.
_Avoid_: subtask, to-do

**Agent Assignment**:
A Forge-owned, durable association between one Record and an externally managed Agent. It records the provider-facing identity and lifecycle of one work attempt; assignments remain as history after an Agent is removed or replaced.
_Avoid_: record agent metadata, agent link

**Assignment Capability**:
An opaque, revocable credential Forge issues for one open Agent Assignment. It grants only that Assignment's permitted agent-initiated operations through the Forge daemon, which is the sole authorization gateway; it is not protection against a hostile process running as the same OS user.
_Avoid_: agent token, agent credential

**Assignment Context**:
The bounded Forge information an Agent Assignment may read: its Task and Records directly related to that Task, including its parent Spec, but not sibling Tasks or transitive relationship traversal.
_Avoid_: project visibility, record graph access

**Agent Profile**:
A versioned Forge-owned configuration selected by exactly one Record Type that specifies how Forge provisions an Agent, including its adapter, model, retry policy, and initial-prompt template.
_Avoid_: agent configuration, record profile

**Agent Workspace**:
The filesystem folder in which an Agent works for an Agent Assignment. V1 uses the scoped Project’s Registered Folder; a future revision will use a task-specific managed Worktree.
_Avoid_: agent project, agent checkout

**Comment**:
A non-blank free-text entry on a Record, optionally attributed to a free-text author. Its content cannot be edited, any Forge client may delete it, and it may be added even after its Record is Done.
_Avoid_: note, message

**Comment Identifier**:
An opaque stable numeric identity that uniquely references a Comment across Forge interfaces.
_Avoid_: comment number

**Record Update**:
A Forge-authored entry recorded atomically with one successful change to a Record, including Record opening, Body edits, lifecycle transitions, Relationship changes, and Comment additions or deletions. It has a timestamp, a type, and only the type-specific references needed to identify the changed entity.
_Avoid_: event, activity

**Record Update Identifier**:
An opaque stable numeric identity that determines the order of Record Updates when timestamps are equal.
_Avoid_: update number

**Update Log**:
The ordered collection of Record Updates for one Record.
_Avoid_: history, audit log

**Record Status**:
A Record’s lifecycle state: Open, In Progress, or Done. A Record opens as Open and in the initial model advances only from Open to In Progress to Done; future Record types may add constraints.
_Avoid_: state

**Record Metadata**:
The structured attributes associated with a Record, conceptually analogous to a Markdown document’s frontmatter and distinct from its Body.
_Avoid_: frontmatter

**Record Body**:
The required, non-blank free-text, usually Markdown, content describing the work represented by a Record. It may be changed until the Record is Done.
_Avoid_: description

**Record Relationship**:
A typed association between two Records. Parent/child is the initial Relationship kind; its permitted changes and participant types are constrained by Record Type, and future kinds may be introduced.
_Avoid_: link

**Completion Resolution**:
An optional immutable, non-blank free-text explanation recorded when a Record becomes Done.
_Avoid_: outcome code

**Parent Record**:
A Record whose work includes the work represented by one or more Child Records through the parent/child Relationship kind. It may become Done only when all of its Child Records are Done at that time, and shares its Project Scope with every Child Record.
_Avoid_: container, owner

**Child Record**:
A Record that represents work forming part of a Parent Record’s work and may itself be a Parent Record.
_Avoid_: subtask

**Project Scope**:
The distinct zero or more Projects to which a Record’s work relates, explicitly selected and fixed when the Record is opened. An empty Project Scope identifies work outside any Project context; a non-empty scope tells agents which Projects may supply relevant context or receive changes; related Parent and Child Records have identical scopes.
_Avoid_: project ownership, project assignment
