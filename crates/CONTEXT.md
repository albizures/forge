# Forge Runtime

The Forge Runtime context is the product domain implemented by Forge's Rust components. It exists to provide Forge's user-facing and supporting capabilities as one cohesive system.

## Language

**Project**:
A Forge-owned unit that is bound to exactly one Repository, has one Registered Folder that is a main Worktree, and to which Specs will belong. A Project is referenced by its immutable Slug.
_Avoid_: repo

**Repository**:
A local Git repository identified by its checkout, not by its remote address. Independent clones are distinct Repositories even when they share a remote; registration requires the Repository to have at least one configured Remote URL, but Repository identity has no cross-Project behavior currently.
_Avoid_: project, remote repository

**Worktree**:
A checked-out folder of a Repository. Only a main Worktree may be registered as a Project; a Repository's other Worktrees are workspaces of that Project rather than separate Projects.
_Avoid_: project folder

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
Forge’s generic primary entity for representing work across multiple workflows. Its common capabilities form the foundation from which more specific Record types, such as Specs, Tasks, and Epics, may later impose additional constraints.
_Avoid_: v1 entity, type

**Record Identifier**:
An opaque positive numeric identity that uniquely references a Record across Forge interfaces.
_Avoid_: record slug, record name

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
A typed association between two Records that may be changed at any time. Parent/child is the initial Relationship kind; future kinds may be introduced.
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
