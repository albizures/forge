# Can a durable agent publish?

Research for [#37](https://github.com/albizures/forge/issues/37) (map
[#26](https://github.com/albizures/forge/issues/26)). Facts only; no policy is recommended here.
Whether to fix this in Wave 1 or keep it accepted is a separate grilling ticket.

Sources are `main` at `c00b076`: Forge's runner (`runner/src`), crates and docs, and the
installed `runner/node_modules/@earendil-works/pi-durable` 1.5.2. The probe
[`runner/test/publication-probe.mjs`](../../runner/test/publication-probe.mjs) (on this branch
only) runs commands through the **real** durable `bash` tool built by `createProfileRuntime`.
It uses a fixture authorization lease and a fake model key file. No model provider is contacted,
and every GitHub check is read-only (`ssh -T`, `git push --dry-run`, `gh auth status`,
`gh api user`). Probe host: Linux, Node 24.18.0, git 2.53.0, gh 2.102.0 (Homebrew), herdr 0.9.5,
run as the operator's own user.

## Summary

**Partly, and only by host accident.** The Publication Check needs both a push and an open
non-draft PR ([`crates/GLOSSARY.md`](../../crates/GLOSSARY.md) "Publication Check";
[`docs/stacks.md`](../../docs/stacks.md) lines 117-119). With today's durable `bash` tool:

| Step | Durable `bash` result on the probe host | Why |
| --- | --- | --- |
| `git push` over **SSH** (`git@github.com:…`) | **Succeeds** (dry-run authenticated, `exit=0`) | OpenSSH finds `~/.ssh` through the passwd entry, not `$HOME`, and the default identity `~/.ssh/id_ed25519` has no passphrase. |
| `git push` over **HTTPS** | **Fails**: `could not read Username for 'https://github.com'` | No `HOME`, so `~/.gitconfig` and its `gh auth git-credential` helper aren't read. No `GH_TOKEN`. |
| `gh` (any command, including `gh pr create`) | **Fails**: `gh: not found` (exit 127) | `PATH` is `<forge-cli-dir>:/usr/bin:/bin`. `gh` lives in `/home/linuxbrew/.linuxbrew/bin`. |
| `gh` by absolute path | **Fails**: `You are not logged into any GitHub hosts` / `gh auth login` (exit 4) | No `HOME`, `GH_TOKEN` or `DBUS_SESSION_BUS_ADDRESS` (the token sits in the keyring). |
| `git commit` | **Fails** as is: `Author identity unknown … unable to auto-detect email address` | No global `user.name`/`user.email`. The agent can still pass `-c user.email=…` or set repo-local config. |
| Push to a local `file` remote | Succeeds | Needs no credentials. |

So a durable agent **cannot create a PR** on this host, and therefore can't complete the
changed-result path of the Publication Check (`forge assignment stack --url …` needs an existing
open non-draft PR). The push half works only when the remote is SSH and the operator's default
SSH key needs neither a passphrase nor an agent. The unchanged-result path
(`forge assignment stack --unchanged`) needs no hosting credentials from the agent, because the
daemon does the verification.

The Herdr backend gets git/gh credentials from the **ambient session of the Herdr server**
(`HOME`, full `PATH`, `SSH_AUTH_SOCK`, `DBUS_SESSION_BUS_ADDRESS`). Forge adds only the four
capability variables there. The durable backend strips all of that.

## 1. What environment the durable `bash` tool gets

Forge builds the tool in `runner/src/profiles.ts`:

- Line 150: `const environment = { PATH: cli ? \`${dirname(cli)}:/usr/bin:/bin\` : '/usr/bin:/bin', LANG: 'C.UTF-8', ...capabilityEnv };`
- Line 156: `createBashTool({ prepare: execution => { execution.cwd = config.cwd; execution.env = { ...environment }; execution.inheritEnv = false; } })`
- Line 143: `capabilityEnv` may hold only the names in `capabilityNames` (line 19):
  `FORGE_ASSIGNMENT_ID`, `FORGE_ASSIGNMENT_DAEMON_URL`, `FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE`,
  `FORGE_ASSIGNMENT_FORGE_CLI`.
- In production, `runner/src/check-runtime.ts` line 38 passes the bootstrapped capability env.
  `runner/src/check-runner.ts` lines 39 and 76-77 require **exactly** those four keys.

pi-durable honours `inheritEnv: false` literally:

- `pi-durable/dist/tools/bash.js` lines 33-39 default `inheritEnv: true`, then run `prepare`.
  Lines 52-55 pass `execution.env` and `execution.inheritEnv` to `env.exec`.
- `pi-durable/dist/env/node.js` lines 205-212, `getShellEnv`:
  `if (!inheritEnv) return { ...extraEnv };`. Neither `process.env` nor the
  `NodeExecutionEnv` `shellEnv` is merged. Lines 742-745 spawn with exactly that env.

So the child environment doesn't depend on the runner process's own environment. The probe
confirms it (`env | sort` in the durable `bash`):

```
FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE=/tmp/forge-publication-probe-…/keys/capability
FORGE_ASSIGNMENT_DAEMON_URL=http://127.0.0.1:9/rpc
FORGE_ASSIGNMENT_FORGE_CLI=/opt/forge-probe/bin/forge
FORGE_ASSIGNMENT_ID=assignment-probe
LANG=C.UTF-8
PATH=/opt/forge-probe/bin:/usr/bin:/bin
PWD=/tmp/forge-publication-probe-…/worktree
```

There is no `HOME`, `USER`, `SSH_AUTH_SOCK`, `GH_TOKEN`/`GITHUB_TOKEN`, `XDG_*`,
`DBUS_SESSION_BUS_ADDRESS` or `GIT_*` variable. The process still runs as the operator's uid,
and `getent passwd` resolves its home (`/home/<user>`).

## 2. What each publication step does in that environment

Raw probe output (durable `bash` vs. an ambient shell of the same user, excerpted; the operator's
name and email are redacted):

```
=== tool lookup
--- durable bash
git=/usr/bin/git
gh=MISSING
ssh=/usr/bin/ssh
forge=MISSING            # the probe's fake FORGE_ASSIGNMENT_FORGE_CLI dir doesn't exist
--- ambient shell
gh=/home/linuxbrew/.linuxbrew/bin/gh

=== git global config visible
--- durable bash
unreadable (exit 128)
no credential/user config
--- ambient shell
file:~/.gitconfig  user.name <redacted>
file:~/.gitconfig  credential.https://github.com.helper !/home/linuxbrew/.linuxbrew/bin/gh auth git-credential

=== git commit with no identity config
--- durable bash
Author identity unknown ... fatal: unable to auto-detect email address (got '<user>@<host>.(none)')
exit=128

=== ssh auth to github.com   (ssh -o BatchMode=yes -T git@github.com)
--- durable bash
Hi <user>! You've successfully authenticated, but GitHub does not provide shell access.

=== git push --dry-run to github (ssh)   (origin = git@github.com:albizures/forge)
--- durable bash
To github.com:albizures/forge
 * [new branch]      HEAD -> research/durable-publication
exit=0

=== git push --dry-run to github (https)   (GIT_TERMINAL_PROMPT=0)
--- durable bash
fatal: could not read Username for 'https://github.com': terminal prompts disabled
exit=128
--- ambient shell
exit=0

=== gh on PATH: auth status
--- durable bash
/bin/sh: 1: gh: not found
exit=127

=== gh by absolute path: auth status
--- durable bash
You are not logged into any GitHub hosts. To log in, run: gh auth login
exit=1
--- ambient shell
✓ Logged in to github.com account <user> (keyring) ... Git operations protocol: https

=== gh by absolute path: api user
--- durable bash
To get started with GitHub CLI, please run:  gh auth login
Alternatively, populate the GH_TOKEN environment variable with a GitHub API authentication token.
exit=4
```

Why SSH works: OpenSSH resolves `~` through the passwd database, not `$HOME`. The probe host's
`~/.ssh/config` sets no `IdentityAgent` for `github.com`, so `ssh -G github.com` falls back to
the default identity files, and `~/.ssh/id_ed25519` loads with an empty passphrase. No agent
socket is involved: the durable env has no `SSH_AUTH_SOCK`. On a host whose key has a
passphrase, needs an agent, or whose remote is HTTPS, the push fails too. The probe didn't test
those hosts; this follows from the env above.

Without the `GIT_TERMINAL_PROMPT=0` used in the probe, an HTTPS push can't prompt either.
pi-durable spawns with `detached: true` (new session, no controlling terminal) and stdin
`ignore` (`env/node.js` lines 742-746).

## 3. What the Publication Check requires of the agent

- `docs/stacks.md` lines 117-119: "The publication agent uses `git`/`gh` to push without force,
  reuse or create an open non-draft PR, and mark a matching draft ready after verification."
- `docs/stacks.md` lines 175-177: hosting is GitHub "through `git` and `gh`", and "missing
  credentials … block explicitly".
- `docs/assignment-context.md` lines 255-262: `forge assignment stack --url <pr-url>` runs only
  for the final Publication Check Assignment, and Forge then verifies the open non-draft PR and
  remote branch. `--unchanged` instead verifies a clean worktree at the prepared input commit.
- The daemon does that verification with its **own** environment:
  `crates/server/src/stacks.rs` lines 130-153 run `gh pr view <n> --repo … --json …`. So
  `--unchanged` and the verification half of `--url` don't depend on the agent's credentials.
  Creating the PR does.
- The durable backend is meant to run Publication Checks. `docs/durable-execution.md` lines
  70-73 say "publication rules all apply unchanged". Durable recovery has Publication-specific
  handling (`crates/server/src/durable_authorization/recovery_worktree_tests.rs` lines 315-330,
  `crates/app/src/durable_execution/recovery/worktree.rs` line 52).
- The real-provider harness `crates/cli/tests/durable_real_provider.rs` has no `push`,
  `gh` or publication step (grep finds no match), so no existing test checks publication.

## 4. How the Herdr backend provides credentials today

- `crates/server/src/herdr.rs` lines 27-37: `capability_launch_environment` contains only the four
  `FORGE_ASSIGNMENT_*` values. Lines 914-917 attach it as `env` to Herdr's `tab.create`. Forge
  passes no git/gh credential of any kind.
- Everything else in the Pi process's environment comes from the Herdr server. The running
  `herdr server` (pid from `pgrep`; names read from `/proc/<pid>/environ`, values not read) has
  `HOME`, `PATH`, `USER`, `SSH_AUTH_SOCK`, `DBUS_SESSION_BUS_ADDRESS` and `XDG_RUNTIME_DIR`. A
  Herdr pane on the probe host (the "ambient shell" column) shows `SSH_AUTH_SOCK` pointing into
  `~/.config/herdr/` (Herdr's agent socket) and the full user `PATH`.
- With that ambient session, the same commands all succeed in a Herdr pane: `gh` on `PATH`,
  `~/.gitconfig` with the `gh auth git-credential` helper, the HTTPS dry-run push, and
  `gh api user` through the keyring token. This is how `git push` and `gh pr create` work for a
  Herdr Pi agent: by inheriting the operator's login session, not through any Forge contract.
  No `GH_TOKEN` is set in either environment.
- How Herdr merges `tab.create` `env` with its own environment is Herdr's behaviour (third-party,
  not in this repo). This was observed from a live pane, not read from Herdr source.

## Reproduce

```sh
git switch research/durable-publication
cd runner && npm ci   # or link an existing runner/node_modules
node test/publication-probe.mjs "$(git rev-parse --show-toplevel)"
```

The `gh` absolute path in the probe (`/home/linuxbrew/.linuxbrew/bin/gh`) is specific to the
probe host. The fixture lease is 30 s of fixture time, and the clock doesn't advance.
