# AGENTS.md

## risoapp

- risoapp is a platform, not an application. It holds strong engines on which other software is built. Applications and their features are built on it, and are not part of it.
- It is written in Rust, with [GPUI](https://gpui.rs/) for the user interface, and follows Zed's design. Read how Zed solves a problem before designing a solution.

## Stages: dev and production

- Code is in one of two stages. **Production**: the user reviewed it and approved it. **Dev**: everything else. New code is dev.
- Only the user moves code to production, after reviewing it.
- Production code uses only production code.
- The user reviews every commit.
- A part is the unit of review and of promotion: one top-level module of a crate, `src/<name>.rs` and the folder `src/<name>/` with everything in it. Its name is `<crate>/<name>`.
- Dev parts are in `src/dev/`, declared in `src/dev.rs`. Production parts are in `src/`. New code goes into `src/dev/`.
- The root file of a crate and `src/dev.rs` hold no code of their own: only module declarations, re-exports, attributes, and `fn main` in a binary. `feature = "dev"` is written only in these files.
- Every crate has a feature `dev`, on by default. Workspace crates depend on each other with `default-features = false`, and a crate's `dev` turns on `<dep>/dev`. So `cargo check -p riso --no-default-features` builds riso without any dev code, and must succeed.
- `production.json` holds a fingerprint of every file of every production part. `cargo test -p riso` fails when a part outside `dev/` is not listed, a listed file differs, or a listed part is gone. When a change needs a production part to change, show the user what changes and why, and stamp the part again only when the user approves.
- To promote a part, after the user approves it: move its module out of `dev/`, then stamp it with `RISO_PROMOTE=<crate>/<part>[,<crate>/<part>…] cargo test -p riso --test stages what_is_outside_dev_is_in_production`.
- Test files have no stage: `tests.rs`, `*_tests.rs`, `testing.rs`, and everything in `tests/`.

## Running

- `cargo run` runs production code only.
- `cargo run dev` runs production code and the one part under review, and nothing else. `UNDER_REVIEW` in `app/riso/src/dev/review.rs` names that part, and the same file adds only what it does. With nothing under review, it opens one empty window.

## The product

- An engine never hardcodes what belongs to the application built on it: its name, its version, the names of its files, the names of its environment variables. The application passes them to the engine at runtime, and an engine derives its own names from them (for example `<name>.log` and `<NAME>_LIFECYCLE`).
- An application reads its product from a build config at build time: `app/riso/product.toml`, or the file named by `PRODUCT_CONFIG=<path>` when it is packaged. `build.rs` gives the values to the code as `PRODUCT_NAME` and `PRODUCT_VERSION`.

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
