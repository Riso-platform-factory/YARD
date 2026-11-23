# new-app: a developer's own application, made from YARD

A developer clones YARD and runs one command. They get their own application in `application/`, with their name, identifier, URL scheme, and icon, that runs on macOS, Windows, and Linux. Nothing of the sample application is in it. YARD's engines, extensions, sample, and tools stay where they are, so that later YARD fixes merge into the developer's repository without touching what the developer wrote.

## What the developer runs

```text
cargo xtask new-app --name "North Star" --identifier com.acme.northstar [--scheme northstar] [--icon brand.png] [--version 0.1.0]
cargo run
```

- `--name` and `--identifier` are required. `--scheme` defaults to the Linux name of the product without its dashes (`North Star` → `northstar`); when that is not a valid scheme (`42 Notes` → `42notes`), the command refuses and asks for `--scheme`. `--version` defaults to `0.1.0`.
- `--icon` is a square PNG of at least 512 × 512 pixels, copied into the application. Without it, the application gets YARD's neutral placeholder icon, a plain grey square that is not the sample's icon, and the command says so in its last line.
- **Everything the build and the bundle would refuse is refused here, before anything is written, naming the flag.** The command runs the same checks, never a copy of them, for all three systems whatever system it runs on:
  - the config reader of `yard_build_config` (identifier, scheme, version);
  - the build script's own checks: the icon decoded in full, not only its header, and the version's numbers for the Windows resource, each at most 65535;
  - the checks `cargo xtask bundle macos`, `linux`, and `windows` make before building: the name and version as file names on each system (`/`, `\`, `:`, `{`, `}`, reserved Windows device names, an empty Linux name), the identifier's 127-character limit of Inno Setup, the length of every file name the packages would have.
  These checks need no build and no tool of the system they are for, so they move, where they are not already, into functions that `new-app` and `bundle` both call. They take the target system and architecture as inputs, and `new-app` runs them for every system and architecture YARD packages (x86_64 and aarch64), since a file name's length depends on it.
- **It refuses what would meet the sample or another crate:** the sample's name, identifier, and scheme, compared without case; a name whose Linux form is `yard`, which would share the sample's folders and its `YARD_HOME` and `YARD_LIFECYCLE` variables; a package name that a workspace crate already has.
- It refuses when `application/` exists, and changes nothing.
- It prints what it made, then the commands to run, test, and bundle the application.

## What it makes

```text
application/                      the developer's; YARD never ships a file here
  Cargo.toml                      package and binary named after the product (North Star → north-star)
  build.rs                        one call: yard_build_config's build script
  product.toml                    name, version, identifier, url_scheme, icon
  icon.png                        the given icon, or the placeholder
  src/main.rs                     fn main: runs yard_app with the product, the extensions, the menus
  src/menus.rs                    the application's menus: the sample's menus today, under the product's name
  README.md                       run, test, bundle; where to add the application's own extensions
```

It also changes the root `Cargo.toml`: `application` joins `members`, and `default-members` becomes `["application"]`. So `cargo run` runs the developer's application, and `cargo run -p yard` runs the sample.

- The binary name is the product's Linux name: the name in lower case, each run of characters other than ASCII letters and digits turned into one `-`, the rule packaging already uses. The packaged names stay as packaging writes them (`North Star.app`, `North Star.exe`, `north-star` on Linux).
- The application starts with the extensions the sample has today (the command palette), listed in `src/main.rs`, where the developer adds or removes them.
- `product.toml` holds the version. The package's version in `Cargo.toml` stays `0.1.0` and is not read.
- The application uses YARD's production code only: it depends on YARD's crates with `default-features = false`, and on `gpui` with `workspace = true`, so it always has YARD's GPUI. It has no `dev` feature and no review mode. YARD's review mode, in a developer's repository, is `cargo run -p yard dev`.
- `PRODUCT_CONFIG` keeps its meaning: whoever sets it builds the application with another config, as `bundle --config` does. Nothing sets it by default. The application is apart from the sample when it is built from its own config; set to the sample's config, it becomes the sample, by request.

**An interrupted run.** The application is written into a temporary folder, renamed to `application/`, and only then is the root `Cargo.toml` replaced, written whole to a temporary file and renamed over the old one. An interruption leaves one of three states: nothing; a temporary folder, which the next run deletes; or `application/` without the workspace change. In the last state the next run refuses, names it, and says to delete `application/` and run again.

## Who owns what

| Path | Owner | On a later YARD update |
|---|---|---|
| `crates/`, `extensions/`, `examples/`, `tooling/`, `docs/`, `app/yard/`, `production.json`, `AGENTS.md`, `README.md`, `.cargo/` | YARD | taken as they come |
| `application/` | the developer | never touched by YARD |
| `Cargo.toml` at the root | YARD; in `members`, the developer owns the `application` entries, and `default-members` is the developer's | merged entry by entry: the developer's entries kept, YARD's taken |
| `Cargo.lock` | shared: it resolves YARD's and the application's crates together | its handling on an update is part of `upstream-updates` |

The developer's own extensions go in `application/extensions/<name>/`, added to `members`.

**YARD's checks cover YARD only.** The stage rules and `production.json` skip `application/`, as they skip `examples/`: both scans in `app/yard/tests/stages.rs`, the ledger and the `dev` feature check. The GPL check keeps covering every member, so the application's crates and its extensions never depend directly on Zed's GPL crates.

## What has to change in YARD first

Today a second application cannot be built from YARD without copying the sample's code:

- The startup lives in the sample (`app/yard/src/startup.rs`). It reads the sample's product, URL scheme, update address, and window icon with `env!`, from the variables the sample's build script sets.
- That build script, which turns `product.toml` into those values, the icons, and the Windows resource, lives in `app/yard/build.rs`.
- `cargo xtask bundle` always builds `-p yard --bin yard`, so it would package the sample under the other application's name.

The build script is production code with no stage today (`build.rs` is not a part). It cannot go through `dev/`: a build script runs before the application, so `cargo run dev` has no way to run a new one beside the old. It therefore moves as it is, straight into a part of `yard_build_config`, and `cargo test -p yard` refuses that part until the user approves its stamp; this is the one exception to "new code goes into `dev/`" in this slice. The startup is a production part; its new form is built in `dev/` beside it and replaces it at promotion, as every change to a production part does.

## The parts

### 1. `yard_build_config/build_script`: the build script, outside the sample

- One function of `yard_build_config`, called from a build script. It does what `app/yard/build.rs` does today: reads `product.toml` (or `PRODUCT_CONFIG`), sets the variables, tracks the config and the icon for rebuilds, writes the window icon on Linux and FreeBSD, and compiles the Windows resource on a Windows host only, warning on another host, as today.
- `app/yard/build.rs` becomes one call. `embed-resource` moves to `yard_build_config`'s Windows dependencies. The tests that include `app/yard/build.rs` by path (`window_icon.rs`) move with the code.
- **Stamps:** `yard_build_config/build_script`, new, with the code as `app/yard/build.rs` has it today.
- **What the review shows:** the sample built before and after, with the same variables, the same window icon, and the same Windows resource (on the Windows runner); `cargo run` with the same trace.

### 2. `yard_app/startup`: the startup, outside the sample

- **A new platform crate, `yard_app`,** whose part `startup` is built in `src/dev/`. It holds the startup of `app/yard/src/startup.rs`, unchanged in behavior and logging: the logs, one instance, URLs, settings, theme, keymap, the update check, windows, and the session. Its public functions stay public, since `examples/screenshot` drives them one by one into a headless renderer: `start_logs`, `init`, `open_window`, `window_options`, and `run`.
- **It reads nothing at build time.** Everything comes from its caller in one value: the product, the URL scheme, the update address, and the window icon. A macro of `yard_app`, defined in the part's module, makes that value from the variables of the caller's build script, so `env!` runs in the application's crate, never in `yard_app`. The window icon is included only when the target is Linux or FreeBSD, by a `cfg` on the item, since the file exists only there.
- **While it is in `dev/`:** `cargo run dev`, with `yard_app/startup` under review, starts the sample through `yard_app`; `cargo run` still starts it through `app/yard/src/startup.rs`.
- **At promotion:** `yard_app/startup` moves out of `dev/`. The sample's `yard/startup` keeps only the sample's composition: its application value, made with the macro, and its extensions; `main`, the review mode, and `examples/screenshot` take them from there, so the screenshot keeps the sample's build script and identity. Both parts are stamped.
- **What the review shows:** `cargo run dev`: the sample, started by `yard_app`, with the same startup groups and events as `cargo run` under the same lifecycle filter, the review's own announcement aside.

### 3. `cargo xtask bundle` packages the application it is given

- `--package <name>` names the Cargo package to build. Without it, `bundle` builds the workspace's one default member: `application` in a developer's repository, `yard` in YARD's. Several default members are refused, asking for `--package`.
- The config defaults to `product.toml` in that package's folder; `--config` still names another.
- It builds that package's one binary and packages the executable Cargo reports for that package and target: the selector that matches `yard` today is generalized, not doubled. A package with no binary, or with more than one, is refused before building.
- `--dev` packages the review mode, which only the sample has: with another package it is refused.
- The stages check before building stays the platform's: `cargo test -p yard --test stages`.
- The runner scripts name `--package yard` wherever they bundle the sample with a test config, so that a default member that is not the sample never pairs a sample config with another binary.
- **What the review shows:** `cargo xtask bundle macos --package north-star` packages the generated North Star application.

### 4. `cargo xtask new-app`

- As described above, in `tooling/xtask`, with the files it writes kept as templates in `tooling/xtask/templates/new-app/`.
- The stage scans skip `application/`.
- **Logging:** a group `new-app` with the name and identifier as fields; a debug line per step (checked, written, renamed into place, workspace changed); a refusal is an `info` with what was refused.
- **What the review shows:** in a fresh clone of YARD, `cargo xtask new-app --name "North Star" …`, then `cargo run`: North Star's window, menu, and folders. Then each refusal: a bad identifier, a scheme that cannot be derived, a version over 65535, a name with `/`, an icon that is not square, the sample's identifier, an existing `application/`.

### 5. On the three systems

The runner checks, in `tooling/runner-checks/` on the throwaway repository, gain one case per system, in its own fresh checkout: `cargo xtask new-app` with a fixture icon, `cargo xtask bundle`, install, start, and open `northstar://…`. The URL is proven delivered as in the packaging checks: the application has no extension for the URL's host, so the shell logs that it received the URL and refused it as `no such extension`, once, in the right process.

| System | What it checks |
|---|---|
| macOS | `CFBundleName`, `CFBundleIdentifier`, `CFBundleURLTypes`, and the icon are North Star's; the URL reaches it |
| Linux | the `.desktop` file's `Name`, `Exec`, `Icon`, and `MimeType` are North Star's; the URL reaches the running app |
| Windows | the installed `.exe`'s `ProductName` and icon are North Star's; the registry's scheme key names it; the URL reaches it |

On each system the sample is bundled as well, with `--package yard`, and installed beside North Star: the two never meet in a folder, a socket, a scheme, or a menu. Each app's checks set and read its own variables (`NORTH_STAR_HOME`, `NORTH_STAR_LIFECYCLE`), and so does every existing check that runs a test config, such as the Linux screenshot, which today sets `YARD_LIFECYCLE` for another product.

## How it is checked

- `yard_build_config`: the build script, run by Cargo on a fixture package in a temporary workspace, sets each variable from the config, makes each icon, and on Windows links the resource.
- `yard_app`: the tests that cover the startup today move with it and pass unchanged; one builds a value from a config that is not the sample's and checks the startup uses it.
- `xtask bundle`: the package chosen from `--package` and from the default member; the refusals for no binary, two binaries, several default members, and `--dev` with another package.
- `xtask new-app`: the files written for each set of flags; the root `Cargo.toml` with only `members` and `default-members` changed; each refusal leaves the repository as it was; each of the three interrupted states and what the next run does; the generated application builds (`cargo check`) in a temporary copy of the workspace.
- The lead runs the sample and a generated application and reads their traces.
- The bundle tests cover package selection, and the runners check the installed application’s identity.

## Order

1. `yard_build_config/build_script`.
2. `yard_app/startup`.
3. `cargo xtask bundle --package`.
4. `cargo xtask new-app`.
5. The runner checks on the three systems.

## Not in this slice

- Taking later YARD fixes into the developer's repository, with how `Cargo.toml` and `Cargo.lock` are merged: the next slice, `upstream-updates`.
- Renaming the sample in place: the sample stays YARD's fixture.
- An interactive mode of `new-app`: flags only.
