# Packaging roadmap

## Goal

Packaging turns an application built on risoapp into a product that a user installs: an application bundle with its name, its identifier, its icon, and its version, built from production code only. After that, it keeps the installed application up to date.

## What is built

- `app/riso/product.toml` holds the product's `name` and `version`. `PRODUCT_CONFIG=<path>` names another file when the application is packaged.
- The engines take every name from the product: the log files, the environment variables, the directories.
- `cargo check -p riso --no-default-features` builds the application without any dev code.

## Decisions already made

- An engine never hardcodes what belongs to the product. Everything a package needs comes from the product config.
- A package holds production code only. It is built without the `dev` feature, and the test of the stages must pass first.
- A second channel, such as a preview beside the stable one, is a second product config. Its own name gives it its own directories and its own log files, so both can be installed side by side.
- The packaging command is a shell script, `script/bundle-macos`, as Zed's `script/bundle-mac`. It has no stage and no unit test: the lead agent checks it by building the bundle, opening it, and reading its logs.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md`, and starts with a short contract in this folder, `docs/packaging/<slug>.md`.
- The bundle is not checked through a test extension. The lead agent builds it, opens it as a user would, and reads its logs.
- The features that run inside the application are checked through a test extension, `examples/package_tour`.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`: `on_open_urls`, `on_reopen`, `restart` | Apache-2.0 | Used directly |
| `http_client`, `reqwest_client` | Apache-2.0 | Can be used to ask for and download an update |
| `script/bundle-mac`, and the bundle sections of `crates/zed/Cargo.toml` | Part of Zed's repository | Reference for the steps of a macOS bundle |
| `auto_update`, `auto_update_helper`, `release_channel`, `cli`, `crashes` | GPL-3.0-or-later | Not UI, so not copied. They are the reference for how Zed checks, downloads, and installs an update |

## Features

Status is one of: planned, dev, production.

| # | Slug | What it adds | What can be seen | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `product-identity` | The product config gains an identifier, in reverse-domain form, and an icon. The product and the names derived from it move into a crate without GPUI that every engine can use. It removes `TODO(product-identity)` | The first line of the log names the identifier | none | none | production |
| 2 | `bundle-macos` | One command builds `<Name>.app` from production code only, with the product's name, identifier, version, and icon | The bundle opens from the Finder under its own name and icon, and writes its logs in the product's directory | `script/bundle-mac`, as reference | 1 | production |
| 3 | `open-urls` | The product config names a URL scheme. Opening such a URL starts the application, or reaches the one that is running, and an extension can handle it through its handle | `open riso://hello` shows the URL in the tour's card | `gpui`: `on_open_urls` | 2 | planned |
| 4 | `update-check` | The application asks an address from the product config for the newest version, and tells the user when there is one | With a test server that offers a newer version, a message appears | `http_client`. `auto_update`, as reference | 2. Window engine: `toasts` | planned |
| 5 | `update-install` | The application downloads the newer version, checks it, and installs it. The new version runs at the next start | The installed bundle replaces itself with the newer one from the test server | `auto_update`, as reference | 4 | planned |

## Later

- Packages for Linux and Windows.
- Signing and notarizing the macOS bundle.
- A command-line launcher that opens files or folders in the running application.
- Sending crash reports. The log engine already writes a panic to the diagnostic log.
