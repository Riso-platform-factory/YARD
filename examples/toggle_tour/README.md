# Extension toggle tour

Run `TOGGLE_TOUR=on cargo run dev`. **Toggle note**, **Hide aside**, and **Extension toggle** appear side by side. Press **Ctrl+Alt+C** (Ctrl+Option+C on macOS). The card's count increases by one per press. A plain `cargo run dev` with `TOGGLE_TOUR` unset leaves the tour inactive.

On macOS with the sample product, edit `~/.config/yard/settings.json`. If you set an absolute `XDG_CONFIG_HOME`, use `$XDG_CONFIG_HOME/yard/settings.json`. To isolate the tour, prefix every run with `YARD_HOME=/tmp/yard-toggle` and edit `/tmp/yard-toggle/config/settings.json`. Keep any other entries in the settings file. The sample product writes `yard.log` and `lifecycle.jsonl` under `~/Library/Logs/yard/`, or under `/tmp/yard-toggle/logs/` with the isolated home. On Windows PowerShell, set `$env:TOGGLE_TOUR = "on"` before running `cargo run dev`.

## Keep layout edits across restarts

1. Start with `TOGGLE_TOUR=on cargo run dev`. Drag **Toggle note** onto **Drop note here**. The note moves into the card's `body` slot. Quit the application.
2. Add `"extensions": {"toggle_tour": false}` to `settings.json` and run `TOGGLE_TOUR=on cargo run dev`. The card is absent. **Toggle note** is back at its startup place beside the aside because the move into the card cannot apply while the card’s extension is off. The trace shows the move was skipped, and the `restored` line has `kept=1`. **Ctrl+Alt+C** has no effect. The trace shows `turned off` for `toggle_tour` with no `ERROR` from disabling it. Click **Hide aside**, verify it disappears, and quit.
3. Check `windows[0].layers["yard.default"]` in `~/Library/Application Support/yard/session.json` on macOS. With the isolated home, check `/tmp/yard-toggle/data/session.json`. The layer contains the move of `toggle-note` into `toggle-card` in slot `body`, followed by the hide of `toggle-aside`. `layout.json` beside it holds the same two operations.
4. Remove the `toggle_tour` entry and run `TOGGLE_TOUR=on cargo run dev`. The card returns with the note inside it. The aside stays hidden.

## Check URL routing on macOS

macOS delivers URL events to an app bundle. Use an isolated product and scheme. Copy `app/yard/icon.png` to `/tmp/icon.png`, and save this config as `/tmp/yard-toggle.toml`.

```toml
name = "yard-toggle"
version = "0.1.0"
identifier = "vn.risotech.yard-toggle"
url_scheme = "yard-toggle"
icon = "icon.png"
```

Run these commands. Use `/tmp/yard-toggle-url/config/settings.json` for this case and inspect `/tmp/yard-toggle-url/logs/lifecycle.jsonl`.

```sh
cargo xtask bundle macos --dev --config /tmp/yard-toggle.toml --env TOGGLE_TOUR=on --env YARD_TOGGLE_HOME=/tmp/yard-toggle-url
open dist/yard-toggle.app
open 'yard-toggle://toggle_tour/hello'
```

The card shows `last URL: yard-toggle://toggle_tour/hello`. Quit, add `"extensions": {"toggle_tour": false}`, and run both `open` commands again. The trace reports `turned off` at startup and refuses the URL with `no such extension` at INFO. The card is absent; **Toggle note** and **Hide aside** are still shown, since they belong to `toggle_host`. **Ctrl+Alt+C** has no effect, and disabling the tour produces no `ERROR`.

## Check the settings and command palette

Use `TOGGLE_TOUR=on cargo run dev` for each case, in a fresh isolated home (for example `YARD_HOME=/tmp/yard-toggle-cases`), so the layout from the walkthrough above does not carry over. Clear the previous case's settings before starting the next one.

| Case | Action | Expected result |
| --- | --- | --- |
| Misspelled name | Start with `"extensions": {"toggle_toru": false}`. | The tour loads and the diagnostic log records one `WARN` naming `toggle_toru` per start. |
| Change while running | Start with the tour enabled. While the window is open, save `"extensions": {"toggle_tour": false}`. | The card stays visible and the key still counts. The trace shows `extensions-setting-changed` naming `toggle_tour` and `applies at the next start`. Quit and restart with the same command. The card is gone; **Toggle note** and **Hide aside** stay, since they belong to `toggle_host`. |
| No effective change | Start with the tour enabled. While the window is open, save `"extensions": {"toggle_tour": true, "toggle_toru": false}`. | The tour stays active. The trace shows a settings reload with no `extensions-setting-changed` group or `applies at the next start` line. |
| Command palette off | Start with `"extensions": {"command_palette": false}`. Press **Cmd+Shift+P** on macOS or **Ctrl+Shift+P** elsewhere. | The palette stays closed. The trace shows `turned off` for `command_palette` and no `ERROR` from disabling it. The tour's count key still works. |
