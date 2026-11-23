# Getting Started

YARD is a Rust platform for building desktop applications on [GPUI](https://gpui.rs/), following Zed's design. It provides engines for layout, windows, settings, themes, key bindings, state persistence, logging, and packaging. Applications are built on top of YARD as compiled-in extensions rather than inside the platform itself.

The sample application lives in `app/yard`.

## Running the Sample Application

Run the production build of the sample application:

```sh
cargo run
```

To keep a run isolated from your regular configuration, data, and logs, set `YARD_HOME` to a temporary directory:

```sh
YARD_HOME=/tmp/yard-test cargo run
```

When `<NAME>_HOME` is set (for the sample application, `YARD_HOME`), YARD stores configuration in `<home>/config`, data in `<home>/data`, and logs in `<home>/logs`. Without it, YARD uses the operating system's standard directories (see [Product Config](./building/product-config.md#application-directories)).

## Stages: Production and Dev

Every crate in YARD separates code into two stages:

- **Production** (`src/`): reviewed and approved code. Production code depends only on production code.
- **Dev** (`src/dev/`): code under development or review, enabled by the default `dev` Cargo feature.

Running the application reflects these two stages:

- `cargo run` runs production code only.
- `cargo run dev` runs production code together with the part currently listed in `UNDER_REVIEW` in `app/yard/src/dev/review.rs`. When nothing is under review, it opens an empty window.

To build or test production code without any `dev` code:

```sh
cargo check -p yard --no-default-features
cargo test --workspace --no-default-features
```
