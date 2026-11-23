# upstream-updates: taking later YARD fixes into an application

A developer made their application with `cargo xtask new-app` in a copy of YARD. YARD keeps changing: engines, extensions, tools, fixes. This slice gives the developer one command that brings a later YARD into their repository on a branch of its own, merged by the ownership rules of [Developing YARD](../development.md#workspace-ownership), checked, and left for them to review. Nothing reaches their own branch until they merge it themselves.

## What the developer runs

```text
cargo xtask update [--ref main] [--remote <url>] [--base <commit>]
cargo xtask update --continue
```

- `--ref` is the YARD branch or tag to take (default `main`). `--remote` is YARD's repository (default YARD's GitHub repository); the command keeps it as the git remote `yard-upstream`, adding it when missing and refusing when it exists with another URL unless `--remote` names that URL. Git's own credentials are used (SSH agent, HTTPS credential helper); a URL is never printed or logged with credentials in it.
- **It refuses, changing nothing:** a working tree with changes (staged, unstaged, or untracked files that are not ignored); a merge, rebase, cherry-pick, or revert in progress; a detached `HEAD`; a shallow repository (it says to run `git fetch --unshallow`); a current branch that is itself an update branch; an update branch for that YARD commit that already exists (it names it); a YARD history that no longer contains the YARD commit last merged (rewritten upstream), naming both commits.
- **Already up to date:** when the YARD commit is already an ancestor of `HEAD`, it says so and does nothing.
- It fetches only `--ref`, with the history needed to find the base, and shows the fetch and each check as they run, since both can take minutes.
- It makes the branch `yard-update/<short YARD commit>` from `HEAD`, checks it out, and merges the YARD commit into it with `git merge`, one merge commit whose message names the YARD commit.
- It applies the root manifest and lock rules below, then checks: `cargo test -p yard` (YARD's own rules; they skip the developer's application, while its GPL check covers every member) and `cargo check --workspace --all-targets --locked` (the host's targets; the other systems are the runners' work).
- It prints the branch, the YARD commits taken, what changed by area (engines, extensions, tools, docs, the sample, the root manifest, the lock file), the YARD files the developer had changed that the merge carried, the checks' results, and how to review and integrate: `git diff <start>..yard-update/…`, then `git checkout <your branch> && git merge yard-update/…`. Integrating with a squash, rebase, or cherry-pick drops YARD's commit from the history, and the next update would merge it again: the command says to merge.
- It never changes the developer's branch, never pushes, and never deletes a branch.

## How the merge follows ownership

| Path | Owner | In the merge |
|---|---|---|
| `crates/`, `extensions/`, `examples/`, `tooling/`, `docs/`, `app/yard/`, `production.json`, `AGENTS.md`, `README.md`, `.cargo/`, `.github/`, `.gitignore`, `.gitattributes` | YARD | git's three-way merge. A developer's change YARD did not touch is kept and listed in the report; where both changed the same lines, or YARD deleted or renamed a file the developer changed, it is a conflict |
| `app/<name>/`, with its `extensions/` | the developer | always the developer's: after the merge, every path under `app/` other than `app/yard/` must be exactly as on the developer's side (paths, modes, content). A YARD commit with anything under `app/` but `app/yard/` is refused before merging |
| other top-level files the developer added | the developer | kept; YARD adding a file of the same name is a conflict |
| Root `Cargo.toml` | shared | see below |
| `Cargo.lock` | shared | see below |

**The root `Cargo.toml`.** The developer owns `default-members` and the `members` entries under `app/<name>/` (the application itself when listed, and its extensions, by path or glob). The result is YARD's new file with those put back: `default-members` as the developer has it, and their `members` entries appended in their order; an application that YARD's `app/*` glob covered and no longer does gets an explicit entry. Any other change the developer made to the root manifest since the base (a `[patch]`, a `[profile]`, a `[workspace.dependencies]` entry, any other key, added, changed, or removed) is a conflict named by key, with the developer's value shown: a dependency belongs in the application's own `Cargo.toml`, and a patch or a profile cannot move there, so the developer decides. Every resolved member and default member must exist after the merge.

**The `Cargo.lock`.** The result starts from YARD's new lock, and Cargo adds the application's crates and their dependencies to it (`cargo metadata` on the merged workspace). A package YARD's lock pins must keep its version and source: when the application's requirements move one, the update stops and names the package, the pin, and the requirement that moved it. The report lists every lock change against YARD's lock and against the developer's previous lock. The checks run with `--locked`, so they cannot change the result reviewed.

**A conflict.** The command stops with the merge in progress on the update branch and lists every conflict, git's and the manifest's. The developer resolves them and runs `cargo xtask update --continue`, which checks the result against the ownership rules again, makes the merge commit, applies the lock rule, and runs the checks. An interruption or a failed check leaves the update branch checked out with what was done; the developer's branch is never touched, so going back is `git checkout <your branch>`.

## A repository made from the GitHub template

YARD is a template repository on GitHub. "Use this template" makes a repository whose first commit holds YARD's files but shares no history with YARD, so git finds no merge base.

- The first update finds the YARD commit the application started from: among the ancestors of the YARD commit to take, the newest whose tree is identical to the tree of the developer's first commit (its root). With `--base <commit>`, the developer names it instead; it must be an ancestor of the YARD commit to take. No match, more than one root commit, or matches that are not on one line of history, and the command refuses and names `--base`.
- On the update branch it first makes a join commit: the developer's tree unchanged, with two parents, `HEAD` and that YARD commit. It changes no file; it records that the application descends from that YARD. The merge of the new YARD then follows as for any repository, with that YARD commit as the base, the same conflicts, and `--continue`.
- From then on the histories are joined and later updates are ordinary.
- A repository made by cloning or forking YARD shares the history already.
- Whether GitHub's copy keeps every tree entry identical (for example `.github/`) is checked on a real repository made from the template, part of this slice's review.

## Logging

A group `yard-update{remote, ref}` around the run (the remote without credentials); a `debug` line per step: `checked`, `fetched`, `base` (how it was found: shared history, tree match, or `--base`), `joined`, `branched`, `merged`, `manifest`, `lock`, `verified`; a refusal is an `info` with what was refused; a conflict is a `warn` listing the paths and keys; a failed check is an `error` in the diagnostic log.

## The parts

### 1. `cargo xtask update` on a shared history

- The command, the refusals, the update branch, the merge, the ownership checks, the root manifest and lock rules, `--continue`, the checks, the report.
- **How it is checked:** tests over temporary git repositories made in the test: a "YARD" repository with a few commits and a "developer" clone of an older one with `new-app` run in it and changes of their own. The update brings the newer YARD and keeps `app/<name>/`, `default-members`, and extension members; takes YARD's lock and adds the application's crates; stops on a moved pin; names a developer's `[patch]` as a conflict; stops on a YARD file both changed and resumes with `--continue`; refuses each refusal; refuses a YARD commit adding to `app/`; does nothing the second time.
- **What the review shows:** a clone of YARD at an older commit, `new-app`, a commit of the developer's, then `cargo xtask update` to today's YARD: the branch, the report, `cargo run` on it.

### 2. Repositories made from the template

- The base found by tree, `--base`, the join commit.
- **How it is checked:** a "developer" repository made as GitHub's template does (YARD's files in a new root commit); the first update joins and merges; the second is ordinary; no match refuses naming `--base`; a wrong `--base` is refused.
- **What the review shows:** the same flow on a repository made from the template on GitHub, with the tree of its first commit compared to YARD's.

### 3. On the runners

The runner checks gain a case per system: clone YARD at the commit before the current one, `new-app`, update to the current one, `cargo xtask bundle`, install, and the North Star case's identity and URL checks on the result. Line endings: the manifests are compared as Git stores them, and the Windows case runs with Git's default `core.autocrlf`.

## Order

1. `cargo xtask update` on a shared history.
2. Repositories made from the template.
3. The runner checks.

## Not in this slice

- Changing a developer's application's own code when YARD changes an API it uses: the update's `cargo check` shows where; the change is the developer's.
- Taking only some YARD changes: an update takes a whole YARD commit.
- Repositories made before `cargo xtask update` exists.
- Pushing, opening a pull request, or merging into the developer's branch.
