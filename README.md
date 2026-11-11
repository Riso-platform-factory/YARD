# risoapp

risoapp is a platform, not an application: strong engines on which other software is built. It is written in Rust, with [GPUI](https://gpui.rs/) for the user interface, and follows the design of [Zed](https://zed.dev/).

## Stages

Code is in one of two stages:

- **Production**: reviewed and approved by the project's owner.
- **Dev**: everything else. New code starts here, in `src/dev/` of its crate.

Production code uses only production code. `production.json` holds a fingerprint of every production file, and the tests fail when production code differs from what was approved.

In production: the log engine, `riso_log`. The application's startup is still dev, so `cargo run` opens no window yet.

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
