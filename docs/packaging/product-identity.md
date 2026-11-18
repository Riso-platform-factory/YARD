# Packaging feature 1: product-identity

## What it adds

The product config gains what a package needs besides the name and the version: an identifier, in reverse-domain form, and an icon. The product and the names derived from it move into a new crate without GPUI, `riso_product`, that every engine can use, so that `riso_paths` stops repeating `riso_log`'s rule for the environment prefix. The first line of the log names the identifier.

## How it works

- **The config.** `app/riso/product.toml` gains two keys:
  - `identifier = "vn.risotech.riso"`: at least two labels separated by dots, each of ASCII letters, digits, and `-`, not starting or ending with `-`; as macOS asks of a bundle identifier.
  - `icon = "icon.png"`: a path relative to the config file, to a square PNG of at least 512 × 512 pixels.
  `build.rs` refuses a missing or invalid value, as it refuses a missing name: the build fails with the file and the key. It gives the code `PRODUCT_IDENTIFIER`, and `PRODUCT_ICON`, the icon's absolute path.
- **The icon.** `app/riso/icon.png`, 1024 × 1024. The lead agent provides it with the contract; it is a placeholder until the user gives the real one.
- **`riso_product`.** A new core crate, without GPUI and without any other crate of the workspace. It holds `Product { name, version, identifier }` and the names derived from the name: the environment prefix, the home variable (`<PREFIX>_HOME`), and the lifecycle variable (`<PREFIX>_LIFECYCLE`). One rule makes the prefix: upper case, and every character that is not an ASCII letter or digit becomes `_`, as both crates do today.
- **Using it.** `riso_log` and `riso_paths` take the product's names from `riso_product`, and keep no rule of their own. `riso_log::Product` and `riso_paths::home_variable` are gone; callers use `riso_product`. The startup builds the product once from `PRODUCT_NAME`, `PRODUCT_VERSION`, and `PRODUCT_IDENTIFIER`, and gives it to both.
- **The first line.** The lifecycle trace begins `run-start riso 0.1.0 vn.risotech.riso (macos)`, and the diagnostic log's first line of a run is `riso 0.1.0 (vn.risotech.riso) starting`.

## What code can do

```rust
// riso_product (dev part `product`)
#[derive(Debug, Clone, Copy)]
pub struct Product {
    pub name: &'static str,
    pub version: &'static str,
    pub identifier: &'static str,
}
impl Product {
    /// Checks the identifier's form, inside the group `product{name, version, identifier}`.
    pub fn new(name: &'static str, version: &'static str, identifier: &'static str) -> Product;
    pub fn environment_prefix(&self) -> String;
    pub fn home_variable(&self) -> String;
    pub fn lifecycle_variable(&self) -> String;
}
```

## Logging

| Situation | Log |
|---|---|
| The startup builds the product | A group `product{name, version, identifier}`, `debug` `checked`. An identifier of the wrong form is an `ERROR` there (the build refuses it first, so it means a bug) |
| The start of a run | `run-start <name> <version> <identifier> (<os>)`, and the diagnostic log's `<name> <version> (<identifier>) starting` |

`riso_product` logs before `riso_log::start` runs: what it logs is kept as the earliest lines are kept today.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_product/product` (new crate, dev) | `Product` and its names |
| `riso_log/diagnostic_log` dev version | Takes `riso_product::Product`; the identifier in the first lines |
| `riso_log/lifecycle_trace` dev version, if `run-start` is written there | The identifier in `run-start` |
| `riso_paths/directories` | Changed when the feature is promoted, not in a dev version: a second `Paths` type would not be the one every other part of `riso_paths` extends. It then takes the home variable from `riso_product`, `home_variable` and its rule are gone, and so is the `TODO(product-identity)`. Until then the dev startup calls `Paths::from_env(product.name)`, whose variable the tests of `riso_product` prove to be the same |
| `riso/startup` and the app's build script | Dev copy that builds the product with its identifier; `build.rs` reads and checks `identifier` and `icon` |

`app/riso/product.toml` and `app/riso/icon.png` are configuration, without a stage.

## Tests

- **`riso_product`, unit tests:** the prefix, home, and lifecycle names for names with spaces, dashes, digits, and non-ASCII letters, as the current tests of both crates check them; identifiers accepted and refused.
- **Build script:** a config without `identifier`, with an invalid one, without `icon`, or with an icon that is missing or not a square PNG of at least 512 pixels fails with the file and the key (run as a test of the build function, not of Cargo).
- **Logs:** `run-start` and the first diagnostic line hold the identifier; the `product` group with `checked`.
- **`AGENTS.md` and the stages test:** `riso_product` has a `dev` feature and stages like every crate.

## Test extension

None: the lead agent runs `RISO_HOME=/tmp/riso-product cargo run dev` and reads the first line of the trace and of `riso.log`.

## Under review

- `riso_product/product`, and `riso_log/diagnostic_log` (with `riso_log/lifecycle_trace` if it changes), `riso/startup` in their dev versions; `riso_paths/directories` as said above

## Not in this feature

- Building the bundle: feature 2, `bundle-macos`, with the script `script/bundle-macos`.
- The URL scheme: feature 3, `open-urls`.
