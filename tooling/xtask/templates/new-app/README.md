# {{name}}

Run these commands from the workspace root.

- Run the application with `cargo run`.
- Test it with `cargo test`.
- Package it with `cargo xtask bundle macos`, `cargo xtask bundle linux`, or `cargo xtask bundle windows` on the corresponding system.

Put your own extensions in `application/extensions/<name>/`. Add each path to the workspace's `members` in the root `Cargo.toml`, add the crate as a dependency in `application/Cargo.toml`, and register its initializer in `src/main.rs`.
