# risoapp

risoapp is a platform, not an application: strong engines on which other software is built. It is written in Rust, with [GPUI](https://gpui.rs/) for the user interface, and follows the design of [Zed](https://zed.dev/).

## Stages

Code is in one of two stages:

- **Production**: reviewed and approved by the project's owner.
- **Dev**: everything else. New code starts here, in `src/dev/` of its crate.

Production code uses only production code. `production.json` holds a fingerprint of every production file, and the tests fail when production code differs from what was approved.

In production:

- the log engine, `riso_log`;
- the application's directories, `riso_paths`: config, data, and logs, under `<NAME>_HOME` when it is set (for example `RISO_HOME`), otherwise in the platform's directories named by the product;
- the UI boundary and theme, `riso_ui`: the only crate that uses Zed's GPL crates; it bundles IBM Plex Sans and Zed's One themes, and gives the UI font and the theme's colors by name, with One Dark as the fixed default;
- the layout model, `riso_layout`, without GPUI: component, node, and slot IDs, and a tree of nodes placed in the named slots of other nodes, under one root;
- the shell, `riso_shell`: each extension, through the handle the startup gives it, registers component types with their slots and places nodes, only of its own namespace and possibly in another extension's slots; the main window's root view creates the view of every node, and each view shows the views of its slots;
- the application's startup, which starts the log first, writes the logs into the logs directory, runs each extension under the name the application gives it, and opens the main window with the shell as its root view.

## Running

```sh
cargo run          # production code only
cargo run dev      # production code and the one part under review
```

## Checking

```sh
cargo test --workspace
cargo check -p riso --no-default-features   # the build without any dev code
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## License

GPL-3.0-or-later.
