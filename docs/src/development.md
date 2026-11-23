# Developing YARD

This section is for contributors working on YARD itself. See `AGENTS.md` at the repository root for the full engineering workflow.

## Workspace Layout

- `crates/`: core platform engines (`yard_product`, `yard_build_config`, `yard_paths`, `yard_log`, `yard_settings`, `yard_layout`, `yard_state`, `yard_keymap`, `yard_ui`, `yard_shell`, `yard_update`, and `yard_app`).
- `extensions/`: platform extensions shipped with YARD (`command_palette`).
- `app/yard/`: the sample application and composition root, plus `app/yard/src/dev/review.rs` for staging parts under review.
- `examples/`: test extensions that exercise engines through the running application (`layout_tour`, `layout_guest`, `layout_inspector`, `keys_tour`, `settings_tour`, `theme_tour`, `window_tour`, `ui_gallery`, `log_tour`, `screenshot`). Example crates have no stage and are never included in production.
- `tooling/xtask/`: workspace automation (`cargo xtask new-app` and `cargo xtask bundle`).

## Stages and Promotion

Code in `crates/`, `extensions/`, and `app/yard/` is split into two stages:

- **Dev** (`src/dev/`, declared in `src/dev.rs`): new code under development or review, compiled when the default `dev` feature is active.
- **Production** (`src/`): reviewed and approved code. Production code may depend only on production code.

`production.json` at the repository root records SHA-256 fingerprints of every file belonging to every production part. `cargo test -p yard` fails if a file outside `dev/` is unlisted, modified, or missing. After approval, moving a part out of `dev/` and stamping it updates `production.json`:

```sh
YARD_PROMOTE=<crate>/<part> cargo test -p yard --test stages what_is_outside_dev_is_in_production
```

Integration tests for a dev part start with `#![cfg(feature = "dev")]` and drop that attribute upon promotion.

## GPL Boundary

`yard_ui` is the only crate in the workspace allowed to depend on Zed's GPL-3.0-or-later crates (`ui`, `theme`, `icons`, `menu`) or hold code adapted from them. It exposes only YARD's own types. The `gpl_boundary` test fails if any other crate adds a GPL dependency from Zed.

## Verification Commands

```sh
cargo check -p yard --no-default-features
cargo test --workspace
cargo test --workspace --no-default-features
cargo clippy --workspace --all-targets
cargo fmt --check
```

## Planning and Design Documents

- [Roadmap](./development/roadmap.md): overview of every engine and the features still to build or waiting by need.
- [Platform Roadmap](./development/platform-roadmap.md): desktop-platform capabilities tracked across the release system, application templates, OS dialogs, and notifications.
- [New App](./development/new-app.md): active contract for separating a developer's application (`application/`) from the sample fixture and adding `cargo xtask new-app`.
- [Packaging](./development/packaging.md): slice 1 contract for cross-platform `cargo xtask bundle` (`macos`, `linux`, `windows`) and signing.
- [Release System](./development/release-system.md): end-to-end design for CI, packaging, signing, publishing, and multi-platform updates.
