# {{name}}

Run these commands from the workspace root.

- Run the application with `cargo run`.
- Run the starter test with `cargo test -p {{package}}`.
- Package it with `cargo xtask bundle macos`, `cargo xtask bundle linux`, or `cargo xtask bundle windows` on the corresponding system.

Put your own extensions in `app/{{package}}/extensions/<extension>/`. Add each path to the workspace's `members` in the root `Cargo.toml`, or add `app/{{package}}/extensions/*` to cover all of them. Add each crate as a dependency in `app/{{package}}/Cargo.toml` and register its initializer in `src/startup.rs`.
