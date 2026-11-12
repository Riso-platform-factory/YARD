# risoapp

risoapp is a platform, not an application: strong engines on which other software is built. It is written in Rust, with [GPUI](https://gpui.rs/) for the user interface, and follows the design of [Zed](https://zed.dev/).

## Stages

Code is in one of two stages:

- **Production**: reviewed and approved by the project's owner.
- **Dev**: everything else. New code starts here, in `src/dev/` of its crate.

Production code uses only production code. `production.json` holds a fingerprint of every production file, and the tests fail when production code differs from what was approved.

In production:

- the log engine, `riso_log`;
- the application's directories, `riso_paths`: config, data, and logs, under `<NAME>_HOME` when it is set (for example `RISO_HOME`), otherwise in the platform's directories named by the product; the names of the files of the user's layout and settings; a check of a file's bytes against the last check; and writing a file in one step, so a crash never leaves half a file;
- the UI boundary and theme, `riso_ui`: the only crate that uses Zed's GPL crates or code copied from them; it bundles IBM Plex Sans and Zed's One themes, gives the UI font and the theme's colors by name, with One Dark as the default and the theme chosen at startup by the settings section `theme` (a mode, dark or light, and a dark and a light theme's name), lays out elements along an axis in shares the user changes by dragging the line between them, adapted from Zed's `pane_group.rs`, and wraps Zed's `Tab` and `TabBar` from its `ui` crate;
- the layout model, `riso_layout`, without GPUI: component, node, and slot IDs, a tree of nodes placed in the named slots of other nodes, under one root, and the default layout kept apart from the user layer, the operations applied after startup; the user layer written to and read from a versioned JSON file;
- the shell, `riso_shell`: each extension, through the handle the startup gives it, registers component types with their slots and places nodes, of its own namespace or of the shell's own containers, `riso.row`, `riso.column`, and `riso.tabs`, and possibly in another extension's slots; while the application runs, the tree changes only through operations (add, remove, move, hide, show) that the engine checks against its rules, refusing a cycle; a hidden or moved node keeps its view; the user drags a tab onto a tab bar or onto the edge of a split's child to move it; an extension zooms one node to fill the window and zooms out, with every view keeping its state; an extension resets the layout to its default; every change of the user layer is saved to the layout file, and at the next start it is applied again over the default layout, skipping with an INFO an operation whose node is gone; the main window's root view creates the view of every node, and each view shows the views of its slots;
- the settings, `riso_settings`: a section is a key and a type whose `Default` gives every value; it is registered once, refused with an ERROR when its key is taken, and read by any code with the `App`; at startup, `settings.json` in the config directory is read once, and its values replace a section's defaults, while a missing or broken file or section keeps the defaults with a WARN; while the application runs, the file is checked every second, and a section whose content changed gets its new value and tells its observers, while a broken edit keeps the last good values;
- the application's startup, which starts the log first, writes the logs into the logs directory, creates the settings store, reads the settings file and watches it, runs each extension under the name the application gives it, and opens the main window with the shell as its root view.

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
