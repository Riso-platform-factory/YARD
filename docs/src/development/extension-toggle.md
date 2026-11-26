# extension-toggle: turning a compiled-in extension off from the settings

An application lists its extensions in its composition root (`EXTENSIONS` in `app/<name>/src/startup.rs`), and they are compiled in. A user who does not want one, for example the command palette, turns it off in `settings.json`, without a rebuild. At the next start the extension's `init` does not run, so nothing of it is registered: no component, action, key binding, URL handler, settings section, or node. The user's own layout around it is kept, so turning it on again brings the layout back as the user left it.

## What the user writes

```json
{
  "extensions": {
    "command_palette": false
  }
}
```

- The section is `extensions`: one entry per extension name, `false` turns it off, `true` or no entry leaves it on. It has the shape of Zed's `auto_install_extensions`.
- **At the next start.** The startup reads the section before it loads the extensions, and skips the `init` of each one turned off. Changing the section while the application runs changes nothing until it starts again, and the trace says so. Turning an extension off while it runs would mean taking back everything it registered, which GPUI and the shell do not do, and Zed does not do for compiled-in features either.
- A name that is not one of the application's extensions is ignored with a `WARN` in the diagnostic log, so a misspelled name is found.
- Only the extensions of the application's list can be turned off. The shell's own namespace `yard`, and the handle the startup makes for update toasts (`update`), are not extensions of the list; the update check follows the product's update address, not this section.
- The section's key is `extensions`, so an extension cannot be named `extensions`: `Extension::new` refuses it, as it refuses `yard`.

## What the user keeps while an extension is off

What the shell knows of an extension that did not load, and what it does today when it meets its traces in the user's files (found in the research for this slice):

- **The layout.** The user's changes are a list of operations (`layout.json`, `layouts/<name>.json`, and each window's layers in `session.json`). When they are restored, an operation that names a node of the turned-off extension, or puts a node in its slot, cannot apply and is skipped with an `info`; today the next save then writes the list without it, within a moment of the start.
  - **Change in this slice:** while any extension is turned off, every operation skipped on restore is kept, in its place in the list, and written back with the user's layer. Operations the user makes in this run come after them. Turning the extension on again replays the whole list, and the layout is as the user left it. Node IDs do not say which extension placed them, so the shell keeps every skipped operation while an extension is off, not only that extension's; one that no longer applies with every extension on is dropped then, as today.
  - Resetting the layout (`reset_layout`) drops the kept operations with the rest of the user's layer, as the user asked.
  - **Change in this slice:** a window whose saved named layout belongs to the turned-off extension shows the default layout and keeps the named layout's name in `session.json`, until the user shows another layout. Today the name is replaced by `yard.default` on the first save.
- **Not changed** (already kept today, or cleared as today):
  - Each node's saved view state (`state.json`) stays waiting and comes back with the node.
  - A tab selection or a zoom that pointed at a node of the turned-off extension is cleared, as today: the window shows another tab, unzoomed.
  - Its store (`store/<extension>.json`) is never opened, and its section in `settings.json` is kept as an unknown section.
  - The user's key bindings to its actions stay in `keymap.json`. GPUI still knows the action, since it is compiled in, and the key does nothing; the command palette does not list it.
  - A URL that names it is refused with `info` `no such extension`. A response to one of its notifications runs nothing, with `info`.

## When extensions depend on each other

YARD does not know that one extension depends on another. When the user turns off an extension that another one uses, the other one meets what it would meet if the extension were not in the list:
- its nodes placed in the turned-off extension's slots are not placed (`info`);
- a component that requires the other extension's context is not placed (`info`);
- reading the other extension's settings section panics, as reading any unregistered section does.

Such an application states in its own documentation which extensions cannot be turned off alone. YARD adds no dependency declarations in this slice.

## Logging

- The group `extensions-load` gains the field `off`, the number of extensions turned off. For each one, its `extension-init{extension}` group logs `info` `turned off` and runs nothing; an extension turned off is the user's choice, not a failure.
- `WARN`, in the diagnostic log: a name in the section that is not one of the application's extensions, once per start.
- When the `extensions` section changes while the application runs, a group `extensions-setting-changed` logs `info` `applies at the next start`, with the names that would change.
- The shell's restore line `restored{applied, skipped}` gains `kept`, the number of skipped operations kept for a turned-off extension.

## Parts

This slice has no dev part of its own. The extensions are loaded by the production part `yard_app/startup`, and the user's layer is kept by the production part `yard_shell/shell`; production code uses only production code, so the change is made in these two parts. It is shown to the user in the review and stamped again only when the user approves, as `os-notifications` did for `yard_app/startup`.

- `yard_app/startup`: the `extensions` section, read before the extensions load; the skip; the `WARN` for unknown names; the line when the section changes; telling the shell that an extension is turned off.
- `yard_shell/shell`: keeping the skipped operations and the named layout's name while an extension is off; refusing the name `extensions`.
- **Acceptance tests**, written before the code by a worker that does not write it, in `crates/yard_app/tests/` and `crates/yard_shell/tests/` (two simulated runs against the same temporary `Paths`, as the existing session tests do):
  - an extension turned off: its `init` does not run, its components are not registered, `extension-init` logs `turned off`, and no `ERROR` is logged;
  - `true` and no entry leave it on; an unknown name gives one `WARN`;
  - a user layer with a move of another node into the turned-off extension's slot, a hidden node of it, and a later edit in the second run: after a run with the extension off, `session.json` and `layout.json` hold every operation in order, followed by the new edit; after a third run with it on, the layout is the one the user left;
  - with every extension on, a skipped operation is dropped as today;
  - a saved named layout of the turned-off extension keeps its name in `session.json`;
  - `reset_layout` drops the kept operations;
  - a change of the section while running logs `applies at the next start` and changes nothing;
  - `Extension::new(cx, "extensions")` is refused;
  - in their own files, the groups, fields, and levels above.
- **Test extension** `examples/toggle_tour`, named in `review.rs` while the change is under review. Without `TOGGLE_TOUR` it does nothing. With `TOGGLE_TOUR=on` it places a card in the window with a slot, binds a key to an action that counts on the card, and handles `<scheme>://toggle_tour/...`. The README walks through: run it and move the card or put another node in its slot; quit; add `"toggle_tour": false` and start again (no card, no `ERROR`, the key does nothing, the URL is refused, the trace says `turned off`); quit; remove the entry and start again (the card is back where the user left it). Also: a misspelled name, a change of the section while running, and turning off `command_palette`.
- **Run by the lead on macOS** with `cargo run dev` and every case. The Linux and Windows runners run the same cases from YARD-testing's `features/extension-toggle/`, since they need only the settings file and the trace.

## Not in this slice

- Turning an extension off or on while the application runs.
- A settings UI for it (roadmap `settings-ui`).
- Declaring that an extension depends on another, and refusing to turn one off alone.
- Keeping a tab selection or a zoom that pointed at a turned-off node.
