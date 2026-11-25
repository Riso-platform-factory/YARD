# Developing YARD

This section is for contributors working on YARD itself. See `AGENTS.md` at the repository root for the full engineering workflow.

## Workspace Layout

- `crates/`: core platform engines (`yard_product`, `yard_build_config`, `yard_paths`, `yard_log`, `yard_settings`, `yard_layout`, `yard_state`, `yard_keymap`, `yard_ui`, `yard_shell`, `yard_update`, and `yard_app`).
- `extensions/`: platform extensions shipped with YARD (`command_palette`).
- `app/yard/`: the sample application and composition root, plus `app/yard/src/dev/review.rs` for staging parts under review.
- `examples/`: test extensions that exercise engines through the running application (`layout_tour`, `layout_guest`, `layout_inspector`, `keys_tour`, `settings_tour`, `theme_tour`, `window_tour`, `ui_gallery`, `log_tour`, `screenshot`). Example crates have no stage and are never included in production.
- `tooling/xtask/`: workspace automation (`cargo xtask new-app`, `cargo xtask bundle`, and `cargo xtask update`).

## Workspace Ownership

| Path | Owner | On a later YARD update |
|---|---|---|
| `crates/`, `extensions/`, `examples/`, `tooling/`, `docs/`, `app/yard/`, `production.json`, `AGENTS.md`, `CLAUDE.md`, `README.md`, `LICENSE`, `.cargo/`, `.github/`, `.gitignore`, `.gitattributes` | YARD | Taken from YARD by `cargo xtask update` ([Upstream Updates](./development/upstream-updates.md)) |
| `app/<name>/` (including `app/<name>/extensions/`) | Developer | Never touched by YARD; YARD ships nothing in `app/` except `app/yard/` |
| Root `Cargo.toml` | Shared | YARD owns workspace configuration and dependencies; the developer owns `default-members` and any `members` entries for `app/<name>/extensions/` |
| `Cargo.lock` | Shared | Resolves YARD's and the application's crates together |

## Stages and Promotion

Code in `crates/`, `extensions/`, `tooling/`, and `app/yard/` is split into two stages:

- **Dev** (`src/dev/`, declared in `src/dev.rs`): new code under development or review, compiled when the default `dev` feature is active.
- **Production** (`src/`): reviewed and approved code. Production code may depend only on production code.

`production.json` at the repository root records fingerprints of every file belonging to every production part. `cargo test -p yard` fails if a file outside `dev/` is unlisted, modified, or missing. After approval, moving a part out of `dev/` and stamping it updates `production.json`:

```sh
YARD_PROMOTE=<crate>/<part> cargo test -p yard --test stages what_is_outside_dev_is_in_production
```

Integration tests for a dev part start with `#![cfg(feature = "dev")]` and drop that attribute upon promotion.

The stage rules in `app/yard/tests/stages.rs` (both the `production.json` ledger and the `dev`-feature and root-file checks) cover YARD's crates only: they skip `examples/` and every workspace member under `app/` other than `app/yard/`, so a developer's application and its extensions under `app/<name>/` do not use stages or `production.json`.

## GPL Boundary

`yard_ui` is the only crate in the workspace allowed to depend on Zed's GPL-3.0-or-later crates (`ui`, `theme`, `icons`, `menu`) or hold code adapted from them. It exposes only YARD's own types. The `gpl_boundary` test (`app/yard/tests/gpl_boundary.rs`) checks every workspace member—including a developer's application and its extensions—and fails if any crate other than `yard_ui` adds a GPL dependency from Zed.

## Verification Commands

```sh
cargo check -p yard --no-default-features
cargo test --workspace
cargo test --workspace --no-default-features
cargo clippy --workspace --all-targets
cargo fmt --check
```

## Runner Checks

`tooling/runner-checks/` (`checks.yml`, `macos.sh`, `linux.sh`, `windows.ps1`) verifies packaging and installed applications on GitHub Actions runners (`macos-latest`, `ubuntu-22.04`, `windows-latest`). Scripts pass `--package yard` wherever they bundle the sample with a test config and set each product's own `<PREFIX>_HOME` and `<PREFIX>_LIFECYCLE` variables.

Each system runs a `northstar` case in a fresh clone of the repository:

1. Runs `cargo xtask new-app --name "North Star" --identifier test.yard.northstar --icon ...` and verifies `app/north-star/` and `default-members`.
2. Bundles North Star without `--package` (using the default member) and bundles the sample with `--package yard`, then installs both side by side.
3. Checks system metadata and icons:
   - **macOS:** `CFBundleName`, `CFBundleIdentifier`, `CFBundleURLTypes`, and the `.icns` icon.
   - **Linux:** `.desktop` `Name`, `Exec`, `Icon`, and `MimeType`.
   - **Windows:** installed `.exe` `ProductName` and icon, plus the registry URL scheme key.
4. Opens `northstar://` to verify URL delivery to North Star's own process (logged once as `no such extension` in its trace), captures a screenshot, and verifies coexistence with the sample (separate install folders, data folders, instance sockets/pipes/mutexes, URL schemes, and application menus, with each URL reaching only its own app).

## Planning and Design Documents

- [Roadmap](./development/roadmap.md): overview of every engine and the features still to build or waiting by need.
- [Platform Roadmap](./development/platform-roadmap.md): desktop-platform capabilities tracked across the release system, application templates, OS dialogs, and notifications.
- [Packaging](./development/packaging.md): slice 1 contract for cross-platform `cargo xtask bundle` (`macos`, `linux`, `windows`) and signing.
- [Release System](./development/release-system.md): end-to-end design for CI, packaging, signing, publishing, and multi-platform updates.
