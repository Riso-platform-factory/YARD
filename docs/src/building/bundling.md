# Bundling

`cargo xtask bundle` packages an application into an installable bundle for macOS, Linux, or Windows directly from its [`product.toml`](./product-config.md).

```sh
cargo xtask bundle <macos|linux|windows> [OPTIONS]
```

> **Note:** `tooling/xtask` currently keeps its bundling commands in `src/dev/` (enabled by its default `dev` feature).

## Common Options

| Flag | Default | Description |
|---|---|---|
| `--package <NAME>` | Workspace's single default member | Application Cargo package to build and bundle |
| `--config <FILE>` | `<package>/product.toml` | Product config file to use |
| `--out <FOLDER>` | `dist/` at repository root | Destination directory for built artifacts |
| `--dev` | off | Build a review bundle running `dev` code (allowed for `yard` only; refused with any other package or if `name`, `identifier`, or `url_scheme` matches `app/yard/product.toml` case-insensitively) |
| `--env KEY=VALUE` | none | Inject an environment variable into a `--dev` bundle (requires `--dev`; refused on Windows) |

Without `--package`, `bundle` selects the workspace's single `default-members` entry (your application after `new-app`, or `yard` in a plain YARD checkout); zero or multiple default members are refused and require `--package`. The selected package must be a workspace member with exactly one binary target (zero or multiple binaries are refused before building), and `bundle` packages the executable Cargo reports for that target.

Before building, `xtask` runs `cargo test -p yard --test stages` to verify YARD's platform stages, strips all signing secret environment variables from `cargo` invocations, and builds the release binary with `--no-default-features` (unless `--dev` is passed).

On `SIGINT`, `SIGTERM`, or `SIGHUP`, `xtask` stops before the next step, cleans up temporary staging folders and keychains without publishing partial artifacts, and exits with status `130`. `xtask` writes its own logs (`xtask.log` and `lifecycle.jsonl`, controlled by `XTASK_LIFECYCLE`) to `$CARGO_TARGET_DIR` when set and non-empty, or `target/` at the repository root otherwise.

## macOS

```sh
cargo xtask bundle macos [--zip]
```

Produces `<out>/<Name>.app`:

- `Contents/MacOS/<Name>`: release executable (or, for `--dev` bundles, a wrapper script that runs `Contents/MacOS/<Name>-bin dev`).
- `Contents/Resources/<Name>.icns`: generated from `icon` at macOS icon sizes (`16` to `512`, `1x` and `2x`).
- `Contents/Info.plist`: populated from `product.toml` (`CFBundleName`, `CFBundleDisplayName`, `CFBundleIdentifier`, `CFBundleShortVersionString`, `CFBundleVersion`, `CFBundleExecutable`, `CFBundleIconFile`, `CFBundlePackageType` set to `APPL`, `CFBundleURLTypes` registering `identifier` as `CFBundleURLName` and `url_scheme` in `CFBundleURLSchemes`, `LSMinimumSystemVersion` `12.0`, `NSHighResolutionCapable`, and `LSEnvironment` for `--dev --env`), validated with `plutil -lint`.
- Signed ad hoc (`codesign --force --deep --sign -`) unless `[release.macos.signing]` is configured.
- In a `--dev` bundle, `<Name>-bin` is the process that runs, so it is signed first under the product's `identifier` (with the entitlements, if any), and the bundle is then signed ad hoc without `--deep` so that identifier stays; with a signing identity, `<Name>-bin` is signed under the product's `identifier` too. macOS notifications identify the app by the running process's signing identifier, and refuse it otherwise. A bundle opened from `/tmp` or another temporary folder is refused notifications too: keep it in `dist/` or the home folder.

Additional outputs:

- `--zip` also writes `<out>/<Name>-<version>.zip` (packed with `ditto`) and prints `sha256 <hex>` for [Updates](./updates.md).
- Setting `dmg = true` under `[release.macos]` also creates a `.dmg` disk image and prints `dmg-sha256 <hex>`.

## Linux

```sh
cargo xtask bundle linux
```

Runs on Linux hosts only. Builds the release binary with `CARGO_PROFILE_RELEASE_STRIP=debuginfo` and produces:

- `<out>/<file_name>.app/`: unpacked application directory containing the binary, `.desktop` entry (registering `url_scheme`), `512 × 512` and `1024 × 1024` icons, and `install.sh`.
- `<out>/<file_name>-<version>-linux-<arch>.tar.gz`: compressed archive of `<file_name>.app/`, printing `sha256 <hex>` and its path.

## Windows

```sh
cargo xtask bundle windows
```

Runs on Windows hosts only. Requires Inno Setup 6 (`ISCC.exe` on `PATH` or in `%ProgramFiles(x86)%\Inno Setup 6`) and Windows `System32\tar.exe` and `System32\certutil.exe`. Produces two artifacts and prints `sha256 <hex>` and the path for each:

- `<out>/<Name>-<version>-<arch>-setup.exe`: Inno Setup installer registering the Start Menu shortcut (with the product's AppUserModelID) and `url_scheme`.
- `<out>/<Name>-<version>-windows-<arch>.zip`: portable archive containing `<Name>/<Name>.exe`.

## Signing and Notarization

Signing is configured under `[release]` in `product.toml`. Secrets are read only from environment variables and never written into logs, arguments, or build scripts.

### macOS Developer ID Signing and Notarization

```toml
[release.macos]
dmg = true
entitlements = "entitlements.plist"

[release.macos.signing]
identity = "Developer ID Application: Example Corp (TEAMID)"
notarize = true
```

Environment variables:

- `MACOS_CERTIFICATE` (base64 `.p12`) and `MACOS_CERTIFICATE_PASSWORD` (optional if the signing identity is already in the local keychain; otherwise imported into a temporary keychain destroyed on exit).
- `APPLE_NOTARIZATION_KEY` (base64 `.p8`), `APPLE_NOTARIZATION_KEY_ID`, and `APPLE_NOTARIZATION_ISSUER_ID` (required when `notarize = true`, which is the default).

### Windows Authenticode Signing

```toml
[release.windows.signing]
timestamp_url = "http://timestamp.digicert.com" # default
```

Environment variables (both required when `[release.windows.signing]` is present):

- `WINDOWS_CERTIFICATE` (base64 `.pfx`).
- `WINDOWS_CERTIFICATE_PASSWORD`.

The `.pfx` is written to a private temporary folder for the run only and removed on exit.
