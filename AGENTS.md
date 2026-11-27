# AGENTS.md

## YARD

- YARD is a platform, not an application. It holds strong engines on which other software is built. Applications and their features are built on it, and are not part of it.
- It is written in Rust, with [GPUI](https://gpui.rs/) for the user interface, and follows Zed's design. Read how Zed solves a problem before designing a solution.

## Workflow

Every change follows this workflow. No step is skipped, and no step starts before the one before it is done.

### Who does what

- **The user** decides, reviews, and approves. Only the user's word commits a change or promotes a part.
  - **Standing approval (2026-11-27), for the platform roadmap's features, taken one at a time in the roadmap's order:** the user does not need to be asked.
    - The lead still writes `.temp/<slug>-proposal.html` and `.temp/<slug>-acceptance.html` with their evidence.
    - The lead approves them itself, by the rules of this file. It also commits, pushes, promotes, and stamps a production part changed as its approved contract says.
    - It stops for the user only for what the work cannot decide: an outside account, money, a licence, or a change of direction.
- **The lead agent** is the agent that talks with the user. It designs, writes the prompts for the coding agent and runs it, reviews, runs the application, writes the documents (`.md` files), and commits. It does not write code.
- **The coding agent** writes all the code, which is every file that is not a `.md` file, from a prompt of the lead agent. It never commits, never pushes, and never edits a `.md` file.

### Workers

The lead agent is Claude. It coordinates Claude and Codex workers through **Orca orchestration** (`orca orchestration`), not by spawning CLI processes or subagents manually (`codex exec`, `gemini.py`, or ad-hoc background shells), and never does a worker's work itself. Antigravity is the one exception: it runs headless, as its row below says. Load the bundled guide (`orca skills get orchestration`) before the first dispatch in a session.

| Worker | How it runs in Orca | What it is given |
|---|---|---|
| **Claude (Opus)** | `orca orchestration worker-start --environment local --worktree current --agent claude --spec-file <prompt>` (select the Opus model with `--model` if not the default) | code, in its turn; reviews |
| **Codex** | `orca orchestration worker-start --environment local --worktree current --agent codex --spec-file <prompt>`; 3 logins inside Orca, listed with `orca account list`: bin.ngoctu@gmail.com (a46c23cd) and yenngan167@gmail.com (05872387, 226d3c4c); switched with `orca account select --agent codex --account <full id>` (applies to launches after it) | code, in its turn; mechanical work: promotions, small fixes from a review; reviews |
| **Antigravity** | Not through Orca: Orca starts `agy` with the real HOME, where no account is signed in, and does not recognize an `agy` started through `account-pool` until a person opens its tab. Accounts are kept by `account-pool` (provider `agy`), each with its own HOME: `account-pool list` shows them (agy-yn yenngan167@gmail.com, agy-tn tu.nguyen@risotech.vn, agy-bn bin.ngoctu@gmail.com, all signed in; take them in turn). Run it headless in print mode, in the background, with permissions bypassed: `zsh -ic 'cd <repo> && agy-yn --dangerously-skip-permissions -p "$(cat <prompt>)"' > ~/riso-codex/out/<task>.txt`. The lead is woken when the command ends and reads the answer from that file. The spec says the worker runs without Orca (no `orca` calls; its final message is the handoff JSON), and that cargo needs `RUSTUP_HOME=/Users/admin/.rustup CARGO_HOME=/Users/admin/.cargo`, since its HOME is the profile's | acceptance tests; reviews; research: reading and summing up documents, code, Zed |

How work is given out:

1. **Code goes round in turn:** the 3 Codex logins one after another (a46c23cd, 05872387, 226d3c4c), then Claude (Opus), then round again, one part each. When a login is at its usage limit or cannot use its model, pass the turn to the next login (or to Claude after the last), and the lead says so in its report.
   - Reports name every account by its email and short id (for example `bin.ngoctu@gmail.com` (a46c23cd)), never by a number: check `orca account list` or `account-pool list` before a dispatch.
2. **Tests and code of one part come from two workers.** Antigravity writes the acceptance tests from the part's contract before the code. The worker whose turn it is writes the code and makes them pass. A worker never writes both.
3. **The reviewer did not write the code:** another Codex login than the writer's, Antigravity, or Claude (Opus). A large change, such as a new design or anything that holds a secret, also gets a second review from a worker of another model, Antigravity or Claude (Opus).
4. **Research that is long but not hard goes to Antigravity:** reading documentation, finding how Zed or a library does something, comparing options. Each answer comes with its source.
5. **Fixes from a review go to the worker that wrote the code,** or to Codex when they are mechanical. Continue a settled worker with `orca orchestration send --to dispatch:<dispatch_id>`, or start a fresh worker when switching workers.
6. **One worker writes a tree at a time.** Reviews and research are read-only (state the read-only rule in their spec) and may run in parallel in `--worktree current`. Use `--worktree new-child` when parallel workers need isolated edits.
7. **Each worker gets a spec file in `~/riso-codex/prompts/`** passed via `--spec-file` (never heredocs) and answers with the handoff JSON (see "A prompt for the coding agent") when reporting `worker_done`. The lead agent's report and the review sheet name which worker wrote the tests, the code, and each review.
8. **Never poll.** After starting worker(s), end the turn and let Orca wake the lead agent. Answer worker questions with `orca orchestration reply`, inspect worker output with `orca orchestration worker-read --dispatch <dispatch_id>` (never read or steer through raw terminal commands), and follow the receipt's `next` step on any failure.

### Before any work

1. Do what was asked, and only that. A request for the log engine is not a request for the paths engine it used elsewhere. When the work needs something that was not asked for, say so and wait.
2. Do not ask again what was already decided. Look in this file, in the repository, and in what the user said before.
3. Change no file that the task does not need, an instruction file least of all.

### The steps of one piece of work

1. **Proposal.** Before any code, the lead agent writes `.temp/<slug>-proposal.html`: what the work adds and why, the design with its sources (Zed first), the parts, how each is checked, and what is left out. Nothing is built until the user approves it.
2. **Foundation.** The coding agent builds the part in `src/dev/`, with its tests and its logging. Nothing is committed.
3. **In the application.** The real application uses the part, in the place where it belongs in production, not in a harness beside the application.
4. **Test extension.** A crate in `examples/` exercises the part through the running application, with a `README.md` that gives each case, its command, and what to look for.
   - Cases are chosen by the extension's own parameters. Without a parameter it does nothing, so a plain `cargo run dev` shows what the application really does.
   - Nothing is staged to look like something else. Every group surrounds work that runs, and every error is one that really happens.
5. **The lead agent's review.** It does not rely on the coding agent's report. Passing tests do not show that the code does the right thing.
   - It reads the whole diff.
   - It runs every check itself: the tests with and without `dev`, the build without `dev`, clippy, the formatter, and new logging tests repeatedly.
   - It runs the application itself, with every case of the test extension, and reads what the application logs and shows.
   - It tries to break what the change must guarantee.
   - It sends every finding to the coding agent in a new prompt, and reviews the result again.
6. **The user's review.** The lead agent reports what changed, how to run each case, what it saw when it ran them, what differs from what was asked, what it did not verify, and what is deferred.
   - The report is `.temp/<slug>-acceptance.html`, every claim with its evidence: screenshots, logs and lifecycle traces, saved files, and the output of each check, kept in `.temp/<slug>-acceptance/` and linked from the page.
   - Then it waits.
7. **Commit**, only after the user approves. A commit holds one concern; a concern the user names for a commit of its own gets one. Never push unless the user asks.
   - **Hand tests on the other systems.** The lead runs the application on one system. Right after the commit, the lead records what it could not run in [YARD-testing](https://github.com/Riso-platform-factory/YARD-testing) (macOS when it ran elsewhere).
     - Each other system gets one issue, from the repository's template, titled `<System>: <slug> (#<n>)`. The issue names the commit where the test extension is under review and lists every case of the extension's README with what that system should show.
     - Each issue gets a row in that repository's README.
     - A feature with a test extension also gets its cases in YARD-testing's `features/<slug>/`, and the workflow **YARD desktop tours** runs them on GitHub's Linux and Windows runners, then posts the results on the issues. A person confirms on a real desktop only what the runner cannot show. The repository's README says how.
     - The report and the feature's roadmap row link the issues.
     - A change with nothing a person must see on those systems, already covered by the runner checks, needs none; the report says so.
     - A failure recorded there is a finding: it becomes an issue in YARD and goes to the workers like a review finding.
     - The roadmap says a system was run only when its issue is closed. Promotion does not wait for these tests.
8. **Promotion**, only when the user says to promote that part, in this order:
   - move the part out of `dev/`, and drop the `dev` line from its tests;
   - leave nothing under review, and take the test extension out of `review.rs`;
   - run the tests once before stamping, to see them refuse the part, then stamp it;
   - check that the moved files are byte for byte what the user approved;
   - run `cargo run` and `cargo run dev`;
   - commit the promotion on its own.
9. **What is deferred** gets a comment at its place: `// TODO(<slug>): <what is missing>. <what unblocks it>.` The slug names the work.

### A prompt for the coding agent

A prompt has, in this order: the goal; the context, with the files to read and the decisions already made; the steps; the constraints; the commands that must pass; and the handoff, one JSON block with `task`, `kind`, `status`, `summary`, `files_changed`, `verification`, `deviations`, and `open_questions`.

- The constraints always say: do not commit, do not push, do not edit a `.md` file, add no dependency that the prompt does not name.
- The coding agent cannot open a window. The prompt tells it not to run the application, and the lead agent runs it.

### Reporting

- Say what is true: a failure with its output, a step that was skipped, a thing that was not verified.
- A rule goes into this file only when it is not here already and the code cannot say it.

## Stages: dev and production

- Code is in one of two stages. **Production**: the user reviewed it and approved it. **Dev**: everything else. New code is dev.
- Only the user moves code to production, after reviewing it.
- Production code uses only production code.
- The user reviews every commit.
- A part is the unit of review and of promotion: one top-level module of a crate, `src/<name>.rs` and the folder `src/<name>/` with everything in it. Its name is `<crate>/<name>`.
- Dev parts are in `src/dev/`, declared in `src/dev.rs`. Production parts are in `src/`. New code goes into `src/dev/`.
- The root file of a crate and `src/dev.rs` hold no code of their own: only module declarations, re-exports, attributes, and `fn main` in a binary. `feature = "dev"` is written only in these files.
- Every crate has a feature `dev`, on by default. Workspace crates depend on each other with `default-features = false`, and a crate's `dev` turns on `<dep>/dev`. So `cargo check -p yard --no-default-features` builds yard without any dev code, and must succeed.
- `production.json` holds a fingerprint of every file of every production part. `cargo test -p yard` fails when a part outside `dev/` is not listed, a listed file differs, or a listed part is gone. When a change needs a production part to change, show the user what changes and why, and stamp the part again only when the user approves.
- To promote a part, after the user approves it: move its module out of `dev/`, then stamp it with `YARD_PROMOTE=<crate>/<part>[,<crate>/<part>…] cargo test -p yard --test stages what_is_outside_dev_is_in_production`.
- Test files have no stage: `tests.rs`, `*_tests.rs`, `testing.rs`, and everything in `tests/`.

## Disk

- Cargo builds into `CARGO_TARGET_DIR` (`~/crew/target`), shared by every checkout.
- After each piece of work, and before starting workers, the lead checks `du -sh "$CARGO_TARGET_DIR"` and `df -h /`.
- When the target folder is over 30 GB, or the disk has less than 15 GB free, the lead first runs `cargo sweep --time 3`, which removes what has not been used for three days. If the limits are still passed, it runs `cargo clean` once. It does either only while no worker is building, and says so in its report.
- A reviewer that builds a scratch copy of the repository gives it its own `CARGO_TARGET_DIR`, for example `/tmp/<review>-target`. The shared folder ignores the checkout's path, so the copy's test binaries would replace the repository's. The reviewer deletes that folder when done.
- `~/.zprofile` keeps builds small for every checkout: `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and `CARGO_INCREMENTAL=0`.

## Running

- `cargo run` runs production code only.
- `cargo run dev` runs production code and the one part under review, and nothing else. `UNDER_REVIEW` in `app/yard/src/dev/review.rs` names that part, and the same file adds only what it does. With nothing under review, it opens one empty window.
- A test extension, in `examples/`, exercises the parts under review through the running app, with cases chosen by its own parameters. Only the list of extensions in `review.rs` names it; the app knows nothing else of it, so removing it from that list and from yard's dependencies leaves the app as it should be. A crate in `examples/` has no stage and is never in production.
- The extensions that come with the platform, such as the command palette, are crates in `extensions/`. They have stages, parts, tests, and logging as the engines in `crates/` do, and `cargo test -p yard` checks them the same way. They reach the platform only through `yard_shell`'s extension handle and the UI kit, and never depend on a GPL crate of Zed. An application's composition root lists the ones it wants.
- An integration test in `tests/` of a part that is not in production starts with `#![cfg(feature = "dev")]`. When the part is promoted, its tests lose that line, and `cargo test --workspace --no-default-features` runs the production parts' tests alone and must pass.

## The product

- An engine never hardcodes what belongs to the application built on it: its name, its version, the names of its files, the names of its environment variables. The application passes them to the engine at runtime, and an engine derives its own names from them (for example `<name>.log` and `<NAME>_LIFECYCLE`).
- An application reads its product from a build config at build time: `app/yard/product.toml`, or the file named by `PRODUCT_CONFIG=<path>` when it is packaged. `build.rs` gives the values to the code as `PRODUCT_NAME` and `PRODUCT_VERSION`.

## Logging

Every crate logs from the start, not as a later addition. No crate and no part is exempt, and an agent never proposes an exemption. There are two logs, both in `yard_log`:

- **Diagnostic log (`log` macros)**: anything that helps diagnose a problem later, kept on disk in `<name>.log`. Use `log::error!` for bugs and failures, `log::warn!` for problems that were handled.
- **Lifecycle trace (`tracing`)**: shows developers what the code is doing, to check it against the design. Never stored beyond the current run.

Rules:

- Wrap each main operation of a crate in a lifecycle group: `tracing::info_span!("<slug>", fields…)`, entered for the duration of the work. The name is a slug that describes the work (such as `layout-resolve`), and instance data goes in fields, not in the name.
- Log the steps inside a group with `tracing::debug!`, and high-frequency work (per frame, per keystroke) only with `tracing::trace!`.
- Log an `ERROR` only for real failures. A handled problem is a `WARN`, and an expected refusal (such as a user action that breaks a rule) is a `tracing::info!`, because every `ERROR` marks its group as failed.
- Async work that belongs to a group is spawned with `.instrument(span)` so its lines stay in that group.
- A group surrounds the work it names while that work runs. Never write a group afterwards to describe work that already ran. `yard_log::start` runs first of all, before the logs directory is known, and keeps what is logged until `yard_log::open_files` can write it, so even the earliest work logs as it runs.
- Quieter is not silent: high-frequency work still logs at `trace`, and a problem logged once in the diagnostic log still appears in the lifecycle trace every time it happens.

Testing logs:

- Logging is part of the behavior, so test that the main operations open their groups and log at the right levels.
- Tests that capture `tracing` output use `tracing::subscriber::with_default`. Do not install a process-wide subscriber in a test that shares its binary with other tests: tests run in parallel in one process, and a global subscriber makes them flaky. Only a test of work that runs on other threads may install one, and it is alone in its own integration test file.
- A test that captures `tracing` output must not run in parallel with another test that reaches the same log lines, even one that captures nothing. `tracing` caches, per log line, whether anyone listens, and a thread without a subscriber that reaches the line first caches "nobody". Either put the test alone in its own integration test file, `tests/<name>.rs`, or make every test of the binary take one shared lock.
- A test that installs a process-wide `log` logger (`log::set_logger`) goes in its own integration test file, `tests/<name>.rs`, which Cargo builds as a separate binary.
- Run new logging tests repeatedly (for example 20 times) before calling them done, to catch flakiness.

---

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
