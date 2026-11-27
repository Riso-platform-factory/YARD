# extension-test-kit: testing an extension without the boilerplate

An extension built on YARD is tested headless, on GPUI's `TestAppContext`. Today every test sets the scene by hand. Across YARD's own 282 test files:

- 67 write their own temporary home;
- 140 create an `Extension` by hand;
- 167 call `yard_ui::init`;
- 150 implement a `tracing` subscriber;
- 134 implement a `log` logger.

Each logging test must also keep the rules in `AGENTS.md` ("Testing logs") that make captured logs reliable. An application's developer has none of this, and the `cargo xtask new-app` template promises `cargo test` without generating a test.

This slice gives extension authors one small crate, `yard_test`, used only as a dev-dependency. It does two things:

- **Boot the application as it really starts**, in an isolated home, with the extensions under test.
- **Capture what it logs**, keeping the rules for them.

The design follows GPUI's own `#[gpui::test]` and `TestAppContext`, which it does not hide. It also follows Zed's `init_test` helpers, which boot just enough of the app for a test. Flutter's `flutter_test`, Compose's test rule, and Tauri's `mock_builder` were read for comparison. The research and sources are in the proposal.

## What an extension's test writes

```rust
use gpui::TestAppContext;
use serde_json::json;

#[gpui::test]
fn the_card_shows_the_saved_name(cx: &mut TestAppContext) {
    let app = yard_test::TestApp::builder()
        .extension("notes", notes::init)
        .settings(json!({ "notes": { "title": "Inbox" } }))
        .boot(cx);

    let window = app.visual(cx);
    // … drive the window with GPUI's own simulate_* calls, read with yard_shell::inspect …

    app.write_settings(cx, json!({ "notes": { "title": "Later" } }));
    // the extension sees the change, as it would at runtime
}

#[test]
fn saving_logs_its_group() {
    yard_test::capture_trace(|trace| {
        // … run the code …
        let group = trace.group("notes-save").expect("the save group");
        assert_eq!(group.fields["count"], "1");
        trace.assert_no_errors();
    });
}
```

## Parts

### `yard_test/harness`: `TestApp`

- **`TestApp::builder()`** returns a `TestAppBuilder`:
  - `.application(yard_app::Application)`, the product, URL scheme, and update URL. The default is a test product, `Test App` `0.0.0`, identifier `test.yard.app`, scheme `yardtest`, and no update URL, so nothing goes to the network.
  - `.extension(name, yard_app::Init)`, once per extension, in the order they start. These are the only extensions; the sample's are not added.
  - `.menus(fn(&Product) -> Vec<gpui::Menu>)`, empty by default.
  - `.settings(serde_json::Value)`, written to the home's `settings.json` before the start.
  - `.boot(cx: &mut TestAppContext) -> TestApp`.
- **`boot`:**
  - needs a fresh `TestAppContext`: one `TestApp` per test. A second `boot` on the same context panics at once, before it changes anything, even when the first one stopped partway, for example on an extension's panic;
  - makes a new temporary home, with `yard_paths::Paths::under`;
  - sets the app identity from the product, as `yard_app::run` does, so system notifications reach GPUI's test platform;
  - writes the settings;
  - runs `yard_app::init` with the application, the extensions, and the menus, exactly as the real start does, so the shell, settings, keymap, store, data file, and HTTP client are all there;
  - opens a new window with `yard_app::open_window`, as `yard_app::run` does after `init`, and draws it. It keeps no session and saves no window bounds, so nothing is left to write into the home after the test;
  - runs until parked.

  It does not call `yard_log::start` (no files are written, and a test captures logs with `logs` below).
- **`TestApp`:**
  - `paths(&self) -> &Paths`, under the temporary home;
  - `window(&self) -> WindowHandle<yard_shell::Shell>`, the window the start opened;
  - `visual(&self, cx) -> VisualTestContext`, on that window;
  - `draw(&self, cx)`, which draws the window and runs until parked;
  - `write_settings(&self, cx, serde_json::Value)`, which writes `settings.json`, advances the test clock by one second so the application's own settings watcher reloads it, and runs until parked. When it returns, the observers of every section that changed and is valid have run. If the test also changed `keymap.json`, the keymap watcher runs in the same second, in either order.
  - The home is deleted when the `TestApp` is dropped, and again when the app quits at the end of the test, in case work still scheduled wrote into it.
- **It hides nothing of GPUI.** The test keeps its `&mut TestAppContext`, with `simulate_*`, the executor, notifications, file dialogs, prompts, and seeds.

### `yard_test/logs`: `capture_trace` and `capture_logs`

- **How it captures.** The first capture in a test binary installs one global `tracing` dispatcher, the kit's router.
  - The router answers `Interest::sometimes()` for every callsite, so no callsite is ever cached as "nobody listens", whichever thread reaches it first. This is the "callsite cache" problem of `AGENTS.md`, solved rather than worked around.
  - It records only on a thread that is running a capture, into that capture. On every other thread it is disabled and costs one check.
  - Right after publishing it, the kit rebuilds `tracing`'s callsite cache, so a callsite another thread reached while the router was being installed is not left cached as "nobody listens".
  - Installing it also turns off `tracing`'s forwarding to `log` (GPUI's Linux dependencies turn that on), since a global dispatcher now exists.
  - If the binary already has a global `tracing` subscriber, or `capture_logs` finds a `log` logger, installation fails before anything is published, neither the router nor the kit's logger, and every later capture repeats that error. A test binary that uses the kit's captures installs no global subscriber or logger of its own. It also does not use `Dispatch::none()` as a scoped default on another thread, since a callsite first reached there can still be cached as "nobody listens".
- **`capture_trace(f: impl FnOnce(&Trace) -> R) -> R`** runs `f` with a new recording on the current thread, and gives it what was logged so far, through `&Trace`.
  - It takes one lock shared by every capture in the test binary, so two captures never interleave. A poisoned lock is taken anyway.
  - A capture inside a capture on the same thread panics at once with a clear message, instead of waiting forever for the lock. A capture inside the test's own `tracing::subscriber::with_default` panics too, since that thread's lines would go to the other subscriber.
  - A thread that waits for another thread that starts its own capture waits for the lock: captures run one at a time.
  - Group ids are unique across captures. A group or line whose explicit parent belongs to another capture gets no parent. Spans of other subscribers are not supported in a capture.
  - `Span::current()` works inside a capture, as it does at runtime: the router reports the current group.
  - It never panics on a group closed late, on another thread, or after the capture: such a close is ignored. A guard should be dropped on the thread that entered it; one dropped elsewhere leaves its group current on the entering thread for the rest of the capture.
- **`capture_logs(f: impl FnOnce(&Trace) -> R) -> R`** does the same, and also captures the diagnostic log (`log` records) of the same thread:
  - It installs one process-wide `log` logger the first time. That logger records only on the thread running `capture_logs`, and only while it runs.
  - It raises the global maximum `log` level for the capture and puts the previous one back after.
  - If the binary already has a `log` logger, the first `capture_logs` panics and says so.
- **What a capture sees is what ran on its own thread.** GPUI's test dispatcher runs the app's foreground and background work on the test thread, so that is all of a `TestApp`'s work. Work on threads the test starts itself is not captured.
- **`Trace`:**
  - `groups() -> Vec<TraceGroup>`, `lines() -> Vec<TraceLine>`, `records() -> Vec<DiagnosticRecord>`;
  - `group(name) -> Option<TraceGroup>` (the first group with that name);
  - `lines_in(&TraceGroup) -> Vec<TraceLine>`;
  - `errors()` and `warnings()`, which take lines and records at that level;
  - `assert_no_errors()`.
- **`TraceGroup { id, parent, name, level, target, fields }`**, **`TraceLine { group, level, target, message, fields }`**, and **`DiagnosticRecord { level, target, message }`**, with `fields: BTreeMap<String, String>`. These are plain `Clone + Debug` data, read after the work ran. A field recorded after a group opened (`Span::record`) is added to it.
- A test that captures nothing still runs in parallel with the others. Two tests that capture take turns.
- **Boot outside the capture when a test checks one operation.** Long-lived groups that boot opens, such as the settings watcher's, keep their lines. A `write_settings` inside the same capture as `boot` has its reload lines under the watcher's group, not under `test-settings-write`.

## Who uses it

- **`yard_test` is a dev-dependency only**, of extensions (`extensions/*`, `app/*/extensions/*`) and applications (`app/*`), with `default-features = false` and its `dev` forwarded as `AGENTS.md` requires. It depends on `yard_app`, so no crate below `yard_app` can use it: their tests keep their own helpers.
- **In the application:** the command palette (`extensions/command_palette`), the extension YARD ships, gets its first test written with the kit. The test boots with the palette, opens it with its key, and checks its group. That test is the slice's use in place.
- **`cargo xtask new-app`** writes a starter test into a new application, after `yard_test` is promoted. A generated application uses `default-features = false` and has no `dev` feature, so it cannot reach a dev part. The steps:
  1. **The template gains a library**, as `app/yard` has one:
     - `src/lib.rs` declares `pub mod menus; pub mod startup;`;
     - `src/startup.rs` holds `pub const EXTENSIONS` (moved from `main.rs`) and `pub fn application() -> yard_app::Application`, which returns `yard_app::application!()`;
     - `src/main.rs` keeps only `main`, calling `yard_app::run(<crate>::startup::application(), <crate>::startup::EXTENSIONS, <crate>::menus::menus)`, where `<crate>` is the package name with `-` turned into `_`.

     So a test reaches the application's real product, extensions, and menus.
  2. **It gains `tests/startup.rs`**, one `#[gpui::test]`. Inside one `capture_logs`, so it sees errors from both logs and from every extension's `init`:
     - it boots `TestApp` with the application's product, each of its `EXTENSIONS` in order, and its menus;
     - it checks that one window opened;
     - it calls `draw` and checks `assert_no_errors`.

     Checking startup as a whole is the one case where the boot goes inside the capture.

     The test is the developer's to change: a starting point, commented as such.
  3. **`Cargo.toml` gains `[dev-dependencies]`:** `gpui` with `test-support`, and `yard_test` with `default-features = false`, as every other YARD dependency of a generated application.
  4. **The template's `README.md`** says that `cargo test -p <package>` runs it.
     `new-app` now refuses a name whose crate name would be `std` or `test`, since the application's library would shadow Rust's own. It also refuses a name that matches, ignoring case and `-`/`_`, a package already in `Cargo.lock` (such as `gpui` or `log`), since `cargo test -p <package>` would then be ambiguous.
  5. **`tooling/xtask/tests/`:**
     - the file list, the `Cargo.toml` sections, and the dependency list (`NEW_APP_DEPENDENCIES`) include the new files and `yard_test`;
     - `new_app_builds.rs` (ignored, run by hand and on the runners) also runs `cargo test -p <app>`. It checks that `opens_one_window_without_errors` ran and passed, and that it fails once the scratch application registers an extension that logs an error in its `init`.

  This is the second commit of the slice, reviewed like the first.

## Logging

- **`test-app-boot{extensions}`** surrounds `boot`. `extensions` is the count, with a `debug` line for the home made, the settings written, and the window drawn.
- **`test-settings-write`** surrounds `write_settings`.
- `logs` logs nothing of its own, since it is what captures.

## Checks

- **Acceptance tests**, written first by a worker that does not write the code, in `crates/yard_test/tests/`:
  - `boot` opens one window, with the extensions' `init` run in order, and the settings visible to an extension through `yard_settings`.
  - Two `TestApp`s in two tests have different homes, and a home is gone after its `TestApp` is dropped.
  - `write_settings` reaches an extension's observer before it returns.
  - `capture_trace` sees a group, its fields, its lines, and their levels. It sees nothing from outside its scope.
  - Two capturing tests in one binary, run 50 times in parallel, never lose a line. This includes a line reached first by an uncaptured thread.
  - `capture_logs` sees a `log::warn!`, and does not see `tracing` events as `log` records.
  - A capture after a panicking capture still works (the lock is poisoned).
  - The kit's own groups, in their own file.
  - Logging tests are run 20 times.
- **The command palette's test** runs with `dev` while `yard_test` is under review, and the palette's other tests pass without it. After promotion it runs in both.

## Known limits

- **An accepted `reveal_path` or `open_path` panics in a test**, because GPUI's test platform does not implement them (`unimplemented!()`). A test can check refusals, and the checks are unit-tested in `yard_shell`. `TODO(extension-test-kit-open-recorder)`: a recorder in `yard_shell/external_open` for tests, [YARD#20](https://github.com/Riso-platform-factory/YARD/issues/20).
- **`Extension::http_client` cannot be faked.** It takes the real client, so a test serves HTTP on a loopback port, as YARD's own tests do. `TODO(extension-test-kit-fake-http)`: a client built on `http_client::FakeHttpClient`, [YARD#21](https://github.com/Riso-platform-factory/YARD/issues/21).
- **`TestPlatform::open_url` keeps only the last URL**, so a test sees one URL at a time.
- **The first capture of a test binary can still miss one callsite.** If another thread reaches a callsite for the first time at the very moment the router is being installed, `tracing` can keep it cached as unused. A test that needs certainty captures once before it starts other threads.
- **A test that panics while a data request is still queued** can leave its home behind: the database worker may create it again after the test, under the system's temporary folder.
- **GPUI's test context has no asset source**, so an icon or image from `yard_ui`'s bundled assets does not load in a test.
- **The settings and keymap watchers poll every second.** On the test dispatcher they run only when the clock advances. `write_settings` advances it by one second, so any other timer due in that second runs too. Reloading directly would need `yard_settings`' private reload made public, which is a production change.

## Not in this slice

- Moving YARD's own 282 tests to the kit.
- A global capture for work on other threads (a test that needs it keeps its own file, as `yard_data` and `yard_http` do).
- A fake file system or database: the home is real and temporary, so the tests run the real atomic writes and SQLite.
- A restart helper (boot, quit, boot again on the same home).
