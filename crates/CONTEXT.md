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
