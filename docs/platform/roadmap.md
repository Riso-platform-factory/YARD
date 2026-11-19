# Platform roadmap: what YARD still lacks

## Goal

The eight steps of `docs/roadmap.md` are done: an application made from YARD is configurable, drawn with Zed's components, used from the keyboard, framed, laid out, remembered, bundled for macOS, opened by URL, and updated. This roadmap lists what a desktop platform is commonly expected to give and YARD does not yet, found while writing the wiki by comparing YARD with Tauri, Electron, Flutter desktop, Wails, and Dioxus, and checked against YARD's code.

## How this roadmap is followed

As every roadmap: features one at a time, in the order of the table, each with a short contract in this folder (`docs/platform/<slug>.md`) approved before any code, and the workflow of `AGENTS.md`. A feature that needs an outside account or a decision of the user (an Apple Developer account, a hosting choice, a licence question) stops at that decision.

## Features

Status is one of: planned, dev, production. Size: S (a day), M (a few days), L (weeks).

| # | Slug | What it adds | Who has it | Size | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `ci-template` | The template ships `.github/workflows/ci.yml`: fmt, clippy with and without `dev`, the tests with and without `dev`, the build without `dev`, the stages and GPL-boundary tests, on a macOS runner with the Metal toolchain. A new application is green on its first push | Tauri's GitHub pipeline | S | none | planned |
| 2 | `new-app` | `script/new-app <name> <identifier>`: renames the sample application (crate, binary, `product.toml`, icon, URL scheme) in one step, so that re-branding is not a hand edit | `create-tauri-app`, `dx new`, `flutter create` | S | none | planned |
| 3 | `release-publish` | `script/release`: builds the bundle with `--zip`, writes `latest.json` (`version`, `url`, `sha256`), and uploads both to a host the product config names. Hosting must not redirect `latest.json` (the update check refuses redirects) | Tauri's updater JSON, Electron Forge publishers | S | 1 | planned |
| 4 | `release-workflow` | `.github/workflows/release.yml`: on a version tag, bundle, zip, publish (feature 3), attach to a GitHub release | `tauri-action` | M | 1, 3 | planned |
| 5 | `developer-id-signing` | `bundle-macos --sign <identity>` with the hardened runtime and entitlements, then notarization and stapling, so that a downloaded application opens without Gatekeeper's block. `update-install` then also requires the running bundle's Team ID | Tauri, Electron, Wails signing guides; Apple notarization | M | an Apple Developer account (the user's) | planned |
| 6 | `signed-updates` | The release file carries a publisher signature (Ed25519) checked against a public key in the product config, so that a compromised host cannot serve a matching malicious `.zip` | Tauri (signatures mandatory), Sparkle (EdDSA) | M | 3 | planned |
| 7 | `dmg` | `bundle-macos --dmg`: a signed, notarized disk image with the drag-to-Applications layout | Tauri DMG, Electron Forge makers | S | 5 | planned |
| 8 | `upstream-updates` | A documented and scripted way for an application made from the template to take later YARD fixes: an `upstream` remote and a merge procedure that re-stamps `production.json`, or engines published as versioned git crates | every framework ships itself as versioned packages | M–L | a decision of the user | planned |
| 9 | `file-dialogs` | Open and save dialogs through the extension handle, wrapping GPUI's `prompt_for_paths` and `prompt_for_new_path`, with logging | Tauri dialog plugin, Electron `dialog` | S | none | planned |
| 10 | `os-notifications` | Notifications in the OS's notification centre when the window is not in front, through the handle | Tauri notification plugin, Electron notifications | S–M | none | planned |
| 11 | `crash-reports` | Native crash dumps beside the panic log, and an opt-in "send report" at the next start | Electron `crashReporter`; Zed's `crashes` (reference only) | M | 15 | planned |
| 12 | `api-docs` | `cargo doc` of the public engine APIs published to GitHub Pages by CI, linked from the wiki | Tauri, Electron, Dioxus API references | S | 1 | planned |
| 13 | `extension-toggle` | A settings section that turns a compiled-in extension off without a rebuild | Zed's extension settings | S | none | planned |
| 14 | `secrets` | An extension keeps a secret in the OS's store of credentials (the state roadmap's `secrets`, by need until now) | Tauri Stronghold, Electron `safeStorage` | S | none | planned |
| 15 | `telemetry-consent` | An opt-in setting and a first-run question for usage and error reports, honoured by every engine that would send anything | Zed's opt-in telemetry | M | none | planned |
| 16 | `tray` | A menu-bar item with a menu built from actions | Tauri system tray, Electron `Tray` | M | none | planned |
| 17 | `autostart` | Start at login, through the handle (`SMAppService`) | Tauri autostart, Electron login items | S | 5 | planned |
| 18 | `accessibility` | Roles and labels on `yard_ui`'s components through GPUI's AccessKit support, checked with VoiceOver | Electron, Flutter, AccessKit | M | none | planned |
| 19 | `settings-ui` | A settings editor and a JSON schema of the declared sections for completion in an editor | Zed's settings UI | M | none | planned |
| 20 | `i18n` | A string catalog, and the UI kit and menus taking keys instead of literal strings | Flutter, Dioxus, Project Fluent | M–L | none | planned |
| 21 | `universal-binary` | `bundle-macos` builds arm64 and x86_64 and joins them with `lipo` | Tauri, Electron | S | none | planned |
| 22 | `linux-windows` | Run, bundle, and update on Linux and Windows: drawn window buttons, AppImage/deb and MSI, single instance and a command-line launcher there | Tauri, Electron, Flutter | L | 1 | planned |
| 23 | `mac-app-store` | A sandboxed build with entitlements and a `.pkg`, without the updater | Tauri, Electron | L | 5 | planned |

## Not planned

- A webview. YARD draws with the GPU through GPUI; that is its point.
- Loading extensions as WebAssembly at runtime, as Zed does: large, and Tauri does not either. `extension-toggle` covers turning one off.
