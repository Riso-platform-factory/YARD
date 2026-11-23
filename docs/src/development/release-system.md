# The release system

One design for how an application made from YARD is checked, built, packaged, signed, published, and updated. The features of the release group of [platform-roadmap.md](./platform-roadmap.md) are slices of it, built in the order given at the end. When the last slice ships, this file is the reference of the release system as it is.

Every choice names where it comes from: Zed (the project YARD follows), or a release tool that already solved the problem (cargo-dist, GoReleaser, Tauri, Electron Forge, electron-builder and electron-updater, Squirrel.Mac, Sparkle, Velopack). What is YARD's own says so.

## YARD is a platform

YARD ships the mechanism; the developer of an application made from it supplies every value in that application's config: its identity, icon, targets, destinations, keys, and secrets. Nothing in the YARD repository is a real product: its `product.toml`, the configs under test, keys, addresses, and repositories exist to test the mechanism. YARD never releases anything itself.

## What the user decided

These stand, and are not asked again:

- macOS, Windows, and Linux are equal targets: every slice is built and checked on all three. Formats follow Zed:
  - macOS: a `.app` in a `.zip`, and later a `.dmg`;
  - Linux: a `.tar.gz` holding the binary, a `.desktop` file, and icons (Zed's `script/bundle-linux`);
  - Windows: an Inno Setup installer `.exe` and a `.zip` (Zed's `script/bundle-windows.ps1`).
- Windows and Linux are checked on GitHub Actions runners (`ubuntu-latest`, `windows-latest`), in a throwaway repository; the lead reads their logs and screenshots.

- Delivery is configured in one place, `[release]` of `product.toml`.
  - Targets: `[release.macos]`, `[release.windows]`, `[release.linux]`.
  - Destinations: `[release.github]`, `[release.pages]`, `[release.s3]`, `[release.ssh]`, `[release.command]`, several at once.
  - `[release.updates]`.
  - What is not configured is skipped.
- Signing is a mechanism: YARD holds no identity. Without `[release.macos.signing]`, the app is signed ad hoc and still released, with notes on how to open it.
- The YARD repository runs CI and no CD: its sample app is not a product. A repository without `[release]` has no CD.
- Releases are trunk-based: a tag on the trunk or on a `release/*` branch releases, after CI passes on the tagged commit.
- Installed copies do not update from GitHub Releases, whose downloads redirect to a host the config does not name. An app on GitHub updates from Pages.
- CD is checked end to end on a throwaway repository made from the template, with test values only.

## The parts

```text
product.toml ──► yard_build_config ──┬──► app/yard/build.rs ──► the app's identity, update address, key
                 (typed config,      │
                  resolved identity) └──► xtask ──► cargo build, Info.plist, codesign, notarytool, ditto
                                              │
yard_feed ───────────────────────────┬──► yard_update (in the app: reads latest.json)
(latest.json, version order,         └──► xtask (writes latest.json, reads it back as the app does)
 limits, address rules, signed bytes)
                                          xtask workflows ──► .github/workflows/*.yml (generated)
```

### Who owns what

| Part | What it owns | Where the idea comes from |
|---|---|---|
| `crates/yard_build_config` | Reading `product.toml` into Rust types with `serde` (`deny_unknown_fields`, enums for closed sets) and checking it. The resolved identity: name, identifier, URL scheme, version, icon path, channel. It reads the config and checks the icon, nothing else | Tauri's typed `tauri.conf.json` |
| `crates/yard_feed` | The contract between a publisher and an installed copy: the `latest.json` type, version order (`semver`), size limits, update-address rules, the signed bytes and their fixtures. No I/O | Tauri and electron-builder: one feed definition shared by both sides |
| `tooling/xtask` | Branch and tag eligibility, packaging, publication, and the generation of workflows. Never linked into the app | Zed's `tooling/xtask`; cargo-dist's `dist` |
| `crates/yard_update` | In the app: check, download, verify, install. Reads the feed through `yard_feed` | Zed's `auto_update` |

- **One rule, one place.** These rules exist twice or three times today, and each moves to one owner:
  - the update-address rules (`yard_build_config`, `install.rs:153`);
  - the packaging values (`script/bundle-macos` reads TOML itself while Cargo reads it through `PRODUCT_CONFIG`).
- **Packaging moves into Rust.** Zed's `script/bundle-mac` is bash because `cargo-bundle` reads Zed's metadata. YARD's metadata is `product.toml`, so the code that resolves it also writes `Info.plist` and lays out the bundle, and calls `codesign`, `notarytool`, `stapler`, and `ditto` directly. A packaged bundle cannot then disagree with the binary inside it. This is YARD's own choice.
  - `script/bundle-macos` becomes `cargo xtask bundle macos`, beside `bundle linux` and `bundle windows`, keeping what it does today:
    - `--config`, `--out`, and `--zip`;
    - the review bundle (`--dev --env`), with its separate name, identifier, and scheme;
    - the stages check before building;
    - the executable path Cargo reports;
    - restoring the previous bundle when a replacement fails.

## Channels

- **Production and preview, from the version.** `1.3.0` is production. A pre-release such as `1.3.0-beta.1` is preview. cargo-dist, GoReleaser, and electron-builder decide it the same way, and so does Zed's tag check (`v1.2.0-pre` is preview). There is one `product.toml` and no file per channel.
- **The preview app is a separate app,** as Zed Preview is. `yard_build_config` resolves the identity once, and `build.rs`, the bundle, and the feed address all take it from there:

  | | Production | Preview |
  |---|---|---|
  | Name | `Notes` | `Notes Preview` |
  | Identifier | `com.acme.notes` | `com.acme.notes.preview` |
  | URL scheme | `notes` | `notes-preview` |
  | Update address | `<url>latest.json` | `<url>preview/latest.json`, joined as a URL, not as text |
  | Each destination's folder | its folder | its folder + `preview/` |
  | GitHub release | normal | pre-release |

  Every name is derived from one source, so two channels cannot collide.
  - A production app refuses a feed whose version is a pre-release, and a preview app refuses a production one. Each reads only its own feed.
  - A local build of a pre-release version is the preview app, with its own data folders, log, and environment variable names, because `yard_product` derives those from the name.
- **Version in the bundle.** macOS wants `CFBundleShortVersionString` to be numbers only. The bundle gets the numeric part (`1.3.0`) there and the full version in `YARDVersion`. The installer compares the full version. Not verified yet on macOS: the slice that adds channels checks it first.
- **Which tag releases what.** The tag must be `v<version>`, exactly as written in `product.toml` on the tagged commit. "On a branch" means reachable from that branch, not at its tip. An annotated tag is read as its commit.

  | Tag | Must be reachable from | Releases |
  |---|---|---|
  | `v1.3.0-beta.1` | `main` | preview |
  | `v1.3.0` | `main` or a `release/*` branch | production |
  | anything else, or another branch | — | nothing: `plan` fails and says why |

  - A version with build metadata (`1.2.3+x`) is refused, since installed copies ignore it when comparing.
  - A version with leading zeros (`01.2.3`) is refused, as the app refuses it.
  - Zed is the same in practice: `main` carries the next version and a `vX.Y.x` branch carries patches. Zed names the channel with a file per branch; YARD uses the version.
- **Not taken from Zed:** nightly builds, which need versions made by the workflow; and a dev channel, since a local build is the version it says.

## The pipeline

As cargo-dist and GoReleaser: each stage reads the records the stage before it wrote, never text written for people or shell state. The local command and the workflow run the same stages in the same order.

```text
plan ─► CI ─► build ─► upload <destination> ─► announce ─► updates ─► prune
 │              │              │                   │           │
plan.json   manifest.json   uploads.json      GitHub release  latest.json,
            + packages      (per destination:  made public     read back over
                             URLs, digests,                    its public URL
                             draft release id)
```

### The stages

1. **plan** is a pure function of the config, the tag, and the branches that hold the tagged commit. It runs on the cheapest runner and refuses before anything is built:
   - a tag not allowed by the table above;
   - a target that is not supported yet;
   - a version older than the one the channel's feed serves (one read of the public feed).

   It writes `plan.json`: the tag, the commit, the channel, the resolved identity, the targets, the destinations with their resolved public addresses, and the update address.
2. **CI** runs the CI workflow on the tagged commit. When it fails, nothing after it runs.
3. **build \<os\>** packages the app from the plan, one job per configured system, on that system's runner.
   - It checks the signing credentials there, the only place that can use them.
   - It writes `manifest.json`: the plan's digest, and each final file (target, role, size, SHA-256).
   - The digest is taken of the zip as it is distributed, after signing and stapling.
4. **upload \<destination\>** runs one job per destination.
   - Each job first checks its own credentials, as GoReleaser's preflight does, but in the job that holds them.
   - It uploads each file and reads it back over its public address, comparing the digest.
   - GitHub gets a **draft** release, as cargo-dist, tauri-action, and Zed do.
   - The job records what it published in `uploads.json`.
5. **announce** makes the GitHub release public, after checking that it holds every file of the manifest. Zed's `validate_release_assets` does the same check.
6. **updates** writes `latest.json` to the `from` destination, last.
   - It runs only after every upload and the announcement passed, so installed copies never hear of a release that is incomplete anywhere. Velopack writes its feed last for the same reason.
   - It then reads the feed back over its public URL, with `yard_feed`'s rules: no redirect, the size limits, the version, the zip's address and digest, and the signature, the way an installed copy reads it.
7. **prune** keeps the current packages and the ones the previous feed named, per channel, and removes older ones. Sparkle and Velopack also prune after the feed. A failed prune leaves extra files and is retried alone.

### Running it again

Stages are idempotent, so re-running the failed jobs of a run finishes it.
- A file already there with the same digest is not uploaded again.
- An existing draft release is reused.
- A feed that already names this version and digest is left as it is.
- The same version with a different digest at any destination stops the release before anything changes. Bump the version instead.
- One release per repository runs at a time.

### Dry run

`cargo xtask release --dry-run` runs plan and build and publishes nothing, as GoReleaser's `--snapshot`. CI does not run it on every push: CI builds the production binary, and packaging runs on tags.

## Destinations

- **First slices:** `github` (the GitHub release of the tag, with `gh`) and `pages` (a commit on `gh-pages`, a folder per channel).
- **Later:** `s3` (with R2 through `endpoint`), `ssh` (`rsync`), and `command`. With them comes one trait, so that adding a host is adding one implementation, as GoReleaser's `Publisher` and electron-builder's providers.
  - Each host states what it can do: serve the feed or not, prune or not.
  - `command` gets a contract for reading back and pruning.
- External commands run through one runner, so tests replace `gh`, `git`, `aws`, and `rsync` with recording fakes.

## Updates and their trust

- **The feed** stays `{version, url, sha256, signature?}`.
- **What installed copies do today, and keep doing:**
  - follow no redirect, at the check or the download;
  - keep the request and size limits, and redact addresses in the logs;
  - do nothing without an update address;
  - refuse a zip whose digest differs, a zip with an entry outside `<name>.app`, or with a link out of it;
  - check the bundle's identifier, version, and signature;
  - replace the bundle at once with an atomic exchange (`renamex_np`); the restart then launches the new version.
- **The signature** is Ed25519 over a framed message of the identifier, the version, and the zip's SHA-256.
  - The format is `yard-update-v1`, then each field as its length and its bytes; the version as written; the digest as 32 raw bytes.
  - `yard_feed` defines the message and holds the fixtures that both the signer and the app test against.
  - A host can then neither swap the package nor pair a new version with an old one: Tauri's "signed version" lesson.
  - The `url` is not signed. The digest binds the bytes wherever they come from.
- **The rules of the key:**
  - A build with a key refuses a feed whose signature is missing or invalid, before it acts on it.
  - `public_key` may name several keys, any one of which verifies. A publisher can then rotate a key: ship a build that trusts the old and the new, then sign with the new one. electron-builder added the same list after shipping a single key.
  - `cargo xtask release keygen` makes a key pair.
  - Zed has no publisher signature because it trusts its own server. A YARD app is served by hosts it does not own.
- **Developer ID.** A Developer ID build also requires the new bundle to carry a valid Developer ID signature with the running bundle's Team ID, as Squirrel.Mac requires the running app's designated requirement.
  - Changing Apple team therefore stops updates, in Squirrel.Mac as here.
  - Moving from ad hoc to Developer ID needs one manual install.
  - Both are written in the reference, not worked around.
- **Where the app runs from.** An app run from a mounted disk image, from a translocated quarantine path, or from a folder the user cannot write is not installable. The app tells the user and downloads nothing, as `update-install` does today. Squirrel.Mac fails in the same places.
- **Replacing a running app.** Today `yard_update` exchanges the bundle while the app runs. Squirrel.Mac waits for the app to quit, with a helper (`ShipIt`). The `release-updates` slice checks the exchange on a real notarized bundle, including resources read after the exchange, before relying on it.
- **One zip per release.** It is built for the build runner's architecture until `universal-binary` makes it universal. The feed has no entry per architecture, which avoids electron-builder's overwritten `latest-mac.yml`.

## The workflows

- **Generated.** `cargo xtask workflows` writes `ci.yml` and `release.yml` with Zed's `gh-workflow`, pinned at Zed's revision. The generator sits behind a feature of `xtask`, so the release jobs do not build it.
- **Checked in CI:**
  - the committed files must equal what the code generates, as Zed's `git diff --exit-code` and cargo-dist's `generate --check`;
  - Zed's two lints: top-level permissions are read-only, and no `${{ }}` appears in a `run:` line.
- **No logic in YAML.** Steps call `xtask`. The plan job builds `xtask` once for Linux and passes the binary to the Linux jobs, so that they do not compile it again; the macOS build job builds its own.
- **The graph.** The run's graph shows plan, CI, build, one upload node per destination, announce, updates, prune. Unconfigured destinations show as skipped nodes.
- **Repositories without `[release]`.** `plan` writes a notice and every other job is skipped. That is the YARD repository itself.

## Errors and logging

- `xtask` follows `AGENTS.md`'s logging rules:
  - one group per stage: `release-plan`, `release-build`, `release-upload` (with the destination as a field), `release-announce`, `release-updates`, `release-prune`;
  - steps at `debug`;
  - expected refusals at `info`.
- Errors carry their context up to `main`. Only `main` prints them, and in Actions it prints them as an annotation.

## Tests

All in the repository, all run by CI on the three runners; what only one system has (codesign, Inno Setup, the `.desktop` file) is tested on that system.

| What | How |
|---|---|
| Config, resolved identity, refusals | Unit tests of `yard_build_config` |
| Feed, version order, signed bytes | Unit tests of `yard_feed`, with fixture files read by the signer and by `yard_update`'s tests |
| Plan and the tag table | Unit tests, with git facts given as data |
| Upload, announce, feed, prune, re-runs | Recording fakes for `gh`, `aws`, `rsync`; a local bare git repository for `pages`; a local HTTP server that answers 200, 404, a redirect, 503, slowly |
| Packaging | Each config under test packaged on its system and read back: the `.app`'s `Info.plist` and signature (`plutil`, `codesign`); the `.tar.gz`'s layout and `.desktop` file (`desktop-file-validate`); the installer installed silently, its registry entries read, then uninstalled. The app started and screenshotted on each runner |
| Workflows | `--check` and the two lints |

End to end, on the throwaway repository, for each slice that publishes: a tag released, an installed copy updated in place, and a re-run of a failed job finishing the release.

## Documents

- This file describes the release system as it will be.
- Each slice has a short contract (`docs/src/development/<slug>.md`) saying what it adds and how it is checked. When the slice ships, what it says moves into this file and the contract is deleted.
- The wiki changes in the same commit as the code.

## Slices, in order

Each slice is built and checked on macOS, Windows, and Linux, ships alone, and leaves releases working:

| # | Slice | What works when it ships |
|---|---|---|
| 1 | `packaging` | `yard_build_config`; `cargo xtask bundle <os> --config <file>` making the `.app`, `.zip`, and `.dmg`, the Linux `.tar.gz`, the Windows installer and `.zip`, from the config alone; signed with the developer's identity when the config names one (macOS Developer ID with notarization, Windows Authenticode), from secrets in the environment. The app runs on all three: window controls drawn as Zed draws them on Linux and Windows, its URL scheme registered and routed, one instance at a time. Proved with several configs under test, each a different app |
| 2 | `ci` | `cargo xtask workflows`, the generated `ci.yml` on the three runners (fmt, clippy, tests with and without `dev`, the production build, stages, GPL boundary, packaging of the configs under test) with `--check` and the lints |
| 3 | `release-local` | `[release]` with its targets, `cargo xtask release plan` and `build` with `--dry-run`, `manifest.json` |
| 4 | `release-workflow` | The generated `release.yml`: plan, CI, a build per configured OS, upload to a GitHub draft, announce. Production versions only |
| 5 | `release-updates` | `yard_feed`, `[release.pages]`, `[release.updates]`, the signature, `keygen`, the `updates` and `prune` stages, re-runs; installed copies update themselves on all three systems, as Zed's `auto_update` does on each |
| 6 | `channels` | Preview: resolved identity, pre-release tags from `main`, `release/*` branches |
| 7 | `release-hosts` | `s3`, `ssh`, `command`, and the destination trait |

`new-app` (renaming the template's app) and `upstream-updates` (merging later YARD changes) are not part of the release system. They stay rows of the roadmap, after slice 1.

## Stages of the code

- `yard_build_config` and `yard_feed` are engines, built in `dev/` as every part.
- `tooling/xtask` is a workspace member, under the same stage rules.
- `build.rs` reads through `yard_build_config`, and `yard_update` through `yard_feed`, only once the user has approved those parts for production.

## Not in this design

- Nightly builds, staged rollouts, delta updates, and package managers such as Homebrew and winget. None is needed by a template without users yet, and each can be added later as a destination or a stage.
- Real notarization, and real S3, R2, and SSH servers, need the publisher's accounts. The lead checks them with fakes and says so.
