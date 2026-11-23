# Getting Started

YARD is a Rust platform for building desktop applications on [GPUI](https://gpui.rs/), following Zed's design. It provides engines for layout, windows, settings, themes, key bindings, state persistence, logging, and packaging. Applications are built on top of YARD as compiled-in extensions rather than inside the platform itself.

The sample application lives in `app/yard`.

## Create an Application

Run these commands from the root of a YARD checkout:

```sh
cargo xtask new-app --name "North Star" --identifier com.acme.northstar [--scheme northstar] [--icon brand.png] [--version 0.1.0]
cargo run
```

| Flag | Required | Default | Notes |
|---|---|---|---|
| `--name` | yes | — | Display name; its Linux name (lowercased, runs of non-ASCII-alphanumeric characters collapsed into `-`, e.g. `North Star` → `north-star`) becomes the folder (`app/<name>/`), Cargo package name, and binary name |
| `--identifier` | yes | — | Reverse-domain identifier (e.g. `com.acme.northstar`) |
| `--scheme` | no | Linux name without `-` (`North Star` → `northstar`) | Refused if the derived value is not a valid URL scheme (for example `42 Notes` → `42notes`), asking for `--scheme` |
| `--version` | no | `0.1.0` | Product version written to `product.toml` |
| `--icon` | no | Neutral grey square placeholder | Square PNG of at least `512 × 512` pixels, copied to `app/<name>/icon.png`. When omitted, YARD copies its grey placeholder (distinct from the sample's icon) and notes `app/<name>/icon.png` in the output |

YARD supports one developer application per repository alongside the sample (`app/yard/`). `cargo xtask new-app` creates `app/<name>/`:

```text
app/<name>/
  Cargo.toml     package and binary named <name>; depends on YARD crates with default-features = false and gpui.workspace = true
  build.rs       one call: yard_build_config::build_script::run()
  product.toml   name, version, identifier, url_scheme, icon = "icon.png"
  icon.png       copied from --icon, or the neutral grey placeholder
  src/main.rs    calls yard_app::run(yard_app::application!(), EXTENSIONS, menus::menus), starting with command_palette
  src/menus.rs   application menus under the product's name
  README.md      run, test, bundle commands and where to add extensions
  extensions/    empty folder for the application's own extensions (see Extensions)
```

The generated crate uses YARD's production code only, has no `dev` feature or review mode, and keeps `version = "0.1.0"` in `Cargo.toml` (`product.toml` holds the product version). Because the root `Cargo.toml` lists `"app/*"` in `workspace.members`, `new-app` adds `"app/<name>"` to `members` only if no existing pattern covers it, and sets `default-members = ["app/<name>"]`. Files are staged in a temporary folder at the repository root (outside `app/`), renamed into `app/<name>/`, and then the root `Cargo.toml` is replaced atomically; leftover staging folders are removed on the next run, and if an interruption leaves `app/<name>/` without updating `default-members`, the next run refuses and tells you to delete `app/<name>/` first.

Before writing anything, `cargo xtask new-app` runs the shared validation checks of `yard_build_config`, the build script, and `bundle` for macOS, Linux, and Windows (`x86_64` and `aarch64`), refusing invalid inputs with the flag name:

- Invalid `product.toml` fields (`name`, `version`, `identifier`, `url_scheme`, `icon`), a PNG that fails full decoding, or a Windows version component (`major`, `minor`, or `patch`) over `65535`.
- A derived `--scheme` that is not a valid URL scheme.
- Names, versions, or identifiers disallowed by packaging on macOS, Linux, or Windows: `/` or `.`/`..`, Windows-forbidden characters (`< > : " / \ | ? *`, control characters, `{`, `}`, leading `@` or `-`, leading/trailing spaces, trailing `.`, reserved device names such as `CON` or `COM1`), an empty Linux name, an identifier over Inno Setup's `127`-character limit, or any generated package file name over `255` units.
- A Cargo package name starting with a digit, matching a reserved Rust or Cargo name, or colliding with an existing workspace crate name (case-insensitively, treating `-` and `_` as equal).
- Collisions with `app/yard/product.toml`: matching `name`, `identifier`, or `url_scheme` (case-insensitively), or a Linux name of `yard`.
- `app/<name>/` already exists, or `default-members` already names an application other than `app/yard`.

After `new-app` updates `default-members`:

- `cargo run` runs your application, `cargo test` tests it, and `cargo xtask bundle <macos|linux|windows>` packages it (see [Bundling](./building/bundling.md)).
- `cargo run -p yard` runs the sample application, and `cargo run -p yard dev` runs YARD's review mode.

Put your application's own extensions in `app/<name>/extensions/` (see [Extensions](./building/extensions.md#application-extensions)).

## Running the Sample Application

Run the production build of the sample application:

```sh
cargo run -p yard
```

To keep a run isolated from your regular configuration, data, and logs, set `YARD_HOME` to a temporary directory:

```sh
YARD_HOME=/tmp/yard-test cargo run -p yard
```

When `<NAME>_HOME` is set (for the sample application, `YARD_HOME`), YARD stores configuration in `<home>/config`, data in `<home>/data`, and logs in `<home>/logs`. Without it, YARD uses the operating system's standard directories (see [Product Config](./building/product-config.md#application-directories)).

## Stages: Production and Dev

YARD's platform crates separate code into two stages:

- **Production** (`src/`): reviewed and approved code. Production code depends only on production code.
- **Dev** (`src/dev/`): code under development or review, enabled by the default `dev` Cargo feature.

Running the sample reflects these two stages:

- `cargo run -p yard` runs production code only.
- `cargo run -p yard dev` runs production code together with the part currently listed in `UNDER_REVIEW` in `app/yard/src/dev/review.rs`. When nothing is under review, it opens an empty window.

To build or test production code without any `dev` code:

```sh
cargo check -p yard --no-default-features
cargo test --workspace --no-default-features
```
