# Extensions

Applications built on YARD are composed of extensions. An extension is a Rust crate compiled into the application and listed in the application's composition root. There is no runtime plugin loading; extensions may depend on each other's crates directly.

Platform extensions that ship with YARD (such as `command_palette`) live in `extensions/`. They reach the platform only through `yard_shell::Extension` and `yard_ui`, and never depend on Zed's GPL crates.

## Application Extensions

An application's own extensions live in `app/<name>/extensions/<extension>/`, one crate per folder. Because the workspace's `app/*` glob in the root `Cargo.toml` does not match nested directories, add each extension path to `workspace.members` (or add `"app/<name>/extensions/*"` to cover all of them). Then add the crate as a dependency in `app/<name>/Cargo.toml` and register its `init` function in `app/<name>/src/main.rs`.

## Initialization and Namespaces

Each extension exports an initialization function:

```rust
pub fn init(cx: &mut gpui::App, ext: &yard_shell::Extension) {
    // register components, settings, actions, keys, URL handlers, and default layout nodes
}
```

The composition root creates a handle per extension with `yard_shell::Extension::new(cx, name)` and passes it to `init`. The handle enforces namespace ownership and load order:

- Component IDs, named layout IDs, window-wide action handlers, settings sections, URL handlers, and extension stores are all scoped to the extension's own `name`.
- The namespace `yard` belongs to the shell.
- When one extension places a node into another extension's slot, it places its own component (or an allowed shell component) and the parent node must already exist—so the contributing extension must be listed after the parent extension in the composition root.
- Violating namespace rules, registering the same resource twice, or passing a malformed identifier is refused with an `ERROR` log.

For registering components, slots, props, contexts, item slots, and named layouts, see [Layout and Components](./extensions/layout-and-components.md).

## Actions and Key Bindings

An extension declares GPUI actions in a namespace matching its extension name:

```rust
gpui::actions!(my_ext, [DoSomething, Ping]);
```

### Binding Default Keys

```rust
ext.bind_keys(
    cx,
    &[
        ("ctrl-alt-i", Box::new(DoSomething), "my_ext"),
        ("ctrl-alt-p", Box::new(Ping), "Shell"),
    ],
);
```

- Extension bindings are registered after the shell's defaults and before [`keymap.json`](../key-bindings.md#user-keymap-file), so extensions can override shell keys and users can override both.
- Invalid keystrokes or key contexts log an `ERROR` and are skipped without blocking the remaining bindings.

### Handling Actions

- **Component-scoped actions:** Track the node's focus handle (`yard_shell::focus_handle(cx, node)`), attach `.key_context("my_ext")`, and handle the action with GPUI's `.on_action(...)` on the component's element. The action runs only while that component has focus.
- **Window-wide actions:** Register a handler on the shell root with `ext.on_action::<Ping>(cx, |action, window, cx| { ... })`. The action must belong to the extension's own namespace. The handler runs inside an `extension-action` trace group regardless of which node has focus.

## Overlays: Toasts, Modals, Prompts, and File Dialogs

Extensions display transient UI above the window layout through their handle:

- **Toasts:** Showing a toast replaces any currently visible toast in that window.
  ```rust
  let toast = yard_ui::Toast::new(yard_ui::ToastKind::Info, "Saved")
      .action("Undo", |window, cx| { /* ... */ });
  ext.show_toast(toast, window, cx);
  ext.hide_toast(window, cx);
  ```
  `yard_shell::has_toast(window, cx)` reports whether a toast is visible.
- **Modals:** Implement `yard_ui::ModalView` (which extends GPUI's `ManagedView` with an optional `fn dims_background(&self) -> bool`, default `false`). Opening a different modal type replaces the active modal; toggling the same type closes it:
  ```rust
  ext.toggle_modal(window, cx, |window, cx| MyModal::new(window, cx));
  ext.hide_modal(window, cx);
  ```
  `yard_shell::has_modal(window, cx)` reports whether a modal is open.
- **Prompts:** Show a native OS dialog with `ext.prompt(prompt, window, cx, callback)`. `Prompt::new` takes button labels as `&[&str]`, and `callback` receives `Option<usize>` (zero-based answer index, or `None` if dismissed without an answer):
  ```rust
  let prompt = yard_shell::Prompt::new(
      gpui::PromptLevel::Warning,
      "Discard changes?",
      &["Discard", "Cancel"],
  )
  .detail("Unsaved changes will be lost.");
  ext.prompt(prompt, window, cx, |answer, cx| {
      // `answer` is Some(index) or None if dismissed
  });
  ```
- **File dialogs:** Choose files or folders with the system's open dialog, or a path to save to with its save dialog. The options are GPUI's `PathPromptOptions`; Linux and Windows cannot mix files and folders, and show a folder chooser when both are asked. Each callback runs once, later, with the paths or `None` (cancelled, dismissed, neither files nor folders asked, or the system could not show the dialog, which is logged as an error). The save dialog only returns a path; the extension writes the file:
  ```rust
  let options = gpui::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None };
  ext.choose_paths(options, cx, |paths, cx| {
      // `paths` is Some(Vec<PathBuf>) or None
  });
  ext.choose_new_path(&directory, Some("Untitled.txt"), cx, |path, cx| {
      // `path` is Some(PathBuf) or None
  });
  ```
  Both run in a `file-dialog{extension, kind}` trace group, and a chosen path never reaches the diagnostic log. File type filters and a starting folder for the open dialog wait for GPUI.

## Settings Sections

An extension declares its own configuration section in [`settings.json`](../settings.md) by implementing `yard_settings::Settings`. Add `serde::Serialize` as well if the section will be updated programmatically via `yard_settings::write`:

```rust
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MySettings {
    pub limit: usize,
}

impl yard_settings::Settings for MySettings {
    const KEY: &'static str = "my_ext";
}
```

Register the section during `init`:

```rust
ext.register_settings::<MySettings>(cx);
```

- `T::KEY` must equal the extension's name; otherwise registration is refused with an `ERROR`. Registering a duplicate section key logs an `ERROR` and retains the first registration.
- Read the current value with `yard_settings::get::<MySettings>(cx) -> &MySettings`.
- Observe live changes with `yard_settings::observe::<MySettings>(cx, |cx| { ... })` (returns a `gpui::Subscription`).
- Update a single field and write `settings.json` atomically with `yard_settings::write::<MySettings>(cx, "limit", serde_json::json!(20))`. `write` requires `T: Settings + serde::Serialize` and validates that the field exists in serialized `T::default()` and deserializes cleanly before writing (see [Settings](../settings.md#live-reload-and-programmatic-writes)).

## Saved State

YARD offers two persistence mechanisms for extensions in the data directory. Application startup configures their paths with `yard_shell::set_state_file(cx, path)` and `yard_shell::set_store_dir(cx, dir)`.

### Per-Node State (`state.json`)

Components can persist small view state per node ID across restarts by implementing `yard_shell::NodeState`:

```rust
impl yard_shell::NodeState for MyView {
    fn save_state(&self, cx: &gpui::App) -> serde_json::Value {
        serde_json::json!({ "count": self.count })
    }

    fn restore_state(
        &mut self,
        state: serde_json::Value,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(count) = state.get("count").and_then(|v| v.as_u64()) {
            self.count = count as usize;
        }
    }
}
```

Register the stateful component during `init`:

```rust
ext.register_state::<MyView>(cx, "my_ext.card");
```

`state.json` uses format version `1`, storing a `windows` array (in session order) where each element maps node IDs to `{"component": "<component_id>", "state": <json>}`:

```json
{
  "version": 1,
  "windows": [
    {
      "card": { "component": "my_ext.card", "state": { "count": 3 } }
    }
  ]
}
```

- Call `yard_shell::state_changed(cx)` whenever the view's persistent state changes. Saves are debounced by `300ms` with a maximum wait of `2s` during continuous edits, written asynchronously on the background executor, and flushed once more on quit (and when closing the last window).
- If `state.json` is unreadable, malformed, or uses an unsupported version, YARD logs a `WARN` and starts without saved state.
- On startup, `restore_state` is called once when the node's view is first created if the saved entry matches the node's current component ID. Views opened in a new window (`yard::NewWindow`) or recreated after removal start from their defaults.
- If a component's registered `NodeState` view type does not match the created view's concrete type, YARD logs an `ERROR` once per node and skips saving and restoring state for it.
- Built-in view state for `yard.tabs` (`selected`) and `yard.window` (`zoomed`) uses the same mechanism in `state.json`. A saved tab selection that is hidden, empty, or gone falls back to the first shown tab; an invalid saved zoom target is cleared.
- Copies created by item slots are not saved in `state.json`.

### Extension Key-Value Store (`store/<extension>.json`)

For small application-wide values that do not belong to a single layout node, use the extension handle's private JSON key-value store:

```rust
let count = ext.stored(cx, "starts").and_then(|v| v.as_u64()).unwrap_or(0);
ext.store(cx, "starts", serde_json::json!(count + 1));
ext.unstore(cx, "obsolete_key");
```

Each store file (`<data>/store/<extension>.json`) uses format version `1` and wraps entries under `"values"`:

```json
{
  "version": 1,
  "values": {
    "starts": 1
  }
}
```

- Keys must be non-empty strings (empty keys are refused with an `INFO` log).
- Each extension can read and write only its own file.
- The file is read lazily on first access, debounced with the same `300ms` / `2s` burst policy on writes, and flushed on quit.
- A missing store file starts empty. A broken, unreadable, or unsupported-version file logs a `WARN` and starts empty, and the next save can replace it.

## Opening URLs

Extensions can handle custom-scheme URLs addressed to them with `ext.on_open_url(cx, handler)`. See [Opening URLs](./opening-urls.md).

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `extensions-load` | `count` | Loading all extensions in startup |
| `extension-init` | `extension` | Running one extension's `init` |
| `extension-action` | `action`, `extension` | Running a window-wide extension action handler |
| `state-register` | `component` | Registering `NodeState` for a component |
| `state-restore` | `file` | Reading `state.json` on startup |
| `state-save` | `windows`, `nodes`, `reason` (`change` or `quit`) | Writing `state.json` |
| `store-read` | `extension`, `file` | Reading `<data>/store/<extension>.json` on first access |
| `store-save` | `extension`, `keys`, `reason` (`change` or `quit`) | Writing `<data>/store/<extension>.json` |
