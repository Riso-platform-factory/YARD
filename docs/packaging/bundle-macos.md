# Packaging feature 2: bundle-macos

## What it adds

One command builds `<Name>.app` from production code only, with the product's name, identifier, version, and icon, as Zed's `script/bundle-mac` builds `Zed.app`. The bundle opens from the Finder under its own name and icon, and writes its logs in the product's own directory.

## How it works

- **The command.** `script/bundle-macos [--config <product.toml>] [--out <folder>]`, a shell script (`bash`, `set -euo pipefail`), run from the repository's root. Without `--config` it uses `app/yard/product.toml`; without `--out`, the folder `dist/` at the root, which git ignores.
- **Production only.** Before building, it runs `cargo test -p yard --test stages` and stops if it fails. It builds with `cargo build --release -p yard --no-default-features`, with `PRODUCT_CONFIG` set to the config, so no dev code is in the binary and the build script checks the config (feature 1).
- **Reading the config.** The script reads `name`, `version`, `identifier`, and `icon` from the config's `key = "value"` lines. A missing key stops it with the file and the key; the build script has the final word on their form.
- **The bundle.** `<out>/<name>.app`, replaced if it exists:
  - `Contents/MacOS/<name>`: the release binary, from cargo's target directory (`cargo metadata` names it);
  - `Contents/Resources/<name>.icns`: made from the icon with `sips` and `iconutil`, at the sizes macOS asks for (16 to 512, each at 1x and 2x);
  - `Contents/Info.plist`: `CFBundleName` and `CFBundleDisplayName` (the name), `CFBundleIdentifier`, `CFBundleShortVersionString` and `CFBundleVersion` (the version), `CFBundleExecutable` (the name), `CFBundleIconFile`, `CFBundlePackageType` `APPL`, `LSMinimumSystemVersion` `12.0` (as Zed's), `NSHighResolutionCapable` true. `plutil -lint` checks it.
  - Signed ad hoc (`codesign --force --deep --sign -`), so that it opens on Apple silicon. Signing with an identity and notarizing are later.
- **What it says.** Each step prints one line (`stages`, `build`, `icon`, `bundle`, `sign`), and the last line is the bundle's path. On a failure it prints the step and exits with a status other than 0, leaving no half-made bundle at the path.

## What the bundle does when opened

Nothing new in the application: it runs production code, so it opens the empty production window, with the product's name in the title bar and the menu. Its directories and logs are the product's (`~/Library/Logs/<name>/<name>.log`), and `<NAME>_HOME` still moves them, as in development.

## Where the code changes

| Part | What changes |
|---|---|
| `script/bundle-macos` (new, no stage) | The command |
| `.gitignore` | `dist/` |

## Checks

The script has no stage and no unit test, as the roadmap decided. The lead agent checks it:

- `script/bundle-macos` builds `dist/yard.app`; `plutil -lint` passes; `codesign --verify` passes; `Info.plist` holds the name, identifier, and version of `product.toml`.
- A second config with another name (`--config` to a temporary file naming `yard-check`, identifier `vn.risotech.yard-check`) gives `dist/yard-check.app`, whose binary's first log line is `yard-check 0.1.0 (vn.risotech.yard-check) starting`.
- Opened with `open --env YARD_HOME=/tmp/yard-bundle dist/yard.app` (so the user's own files are not touched): the window shows, the Dock shows the icon and the name, and `/tmp/yard-bundle/logs/yard.log` begins with `yard 0.1.0 (vn.risotech.yard) starting`. A screenshot of the Finder and of the Dock shows the icon.
- A config with a missing key, or a failing stages test, stops the script with the step and leaves no bundle.

## Not in this feature

- Signing with a developer identity, notarizing, and a `.dmg`: later, as the roadmap says.
- URL schemes in `Info.plist`: feature 3, `open-urls`.
