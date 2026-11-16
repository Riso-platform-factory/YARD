# Layout feature 30: named-layouts

## What it adds

An extension can register named layouts: each is a list of operations over the default layout, such as one arrangement for writing and one for reviewing. A window shows one named layout at a time and can switch to another. Switching keeps the views of the nodes that both layouts show, so a card keeps its state. Each named layout has its own user layer and its own file: what the user changed in one stays there when the window shows another.

## How it works

- **The default.** The default layout of features 1 to 29 is the named layout `riso.default`. It always exists, a window starts with it, and its user layer is the one saved in `layout.json`, as today.
- **Registering.** `Extension::named_layout(cx, name, build)` registers a named layout `name`. `build` receives a `NamedLayout` and calls on it the same operations an extension calls on the layout: `add`, `add_with_props`, `remove`, `move_node`, `set_hidden`, `set_props`, with the same arguments. They are recorded, not applied: a named layout is applied only when a window switches to it.
  - The name is an ID in the extension's namespace (`<extension>.<slug>`), as a component's is. It is refused with an `ERROR` for an invalid name, a name outside the namespace, `riso.default`, or a second registration of the same name.
  - Operations are recorded in order with the extension that registered the layout, so the namespace rule of `add` is checked as for a live operation.
- **Switching.** `Extension::show_layout(window, cx, name)` makes the window show `name`. The window's tree becomes the default layout, then the named layout's operations, then that layout's saved user layer, each operation checked by the shell's rules as at restore (feature 12): a refused operation is skipped with an `INFO`, and the rest is applied. Then:
  - a node whose ID is in the tree before and after keeps its view and its state, even when it moved, as a reset keeps it;
  - a node that leaves has its view dropped (`view dropped`), and a node that comes has its view created;
  - zoom ends, as a reset ends it; focus stays on its node if the node is still shown, else goes to the window;
  - switching to the layout already shown changes nothing and logs `debug` `already shown`;
  - an unknown name is refused with an `INFO`, and the window keeps its layout.
- **What the window shows.** `riso_shell::shown_layout(cx, window)` gives the name of the layout the window shows. The status of the switch is view state: after a restart the window shows `riso.default`.
- **User layers.** Every operation the user or an extension makes while a named layout is shown is recorded in that layout's user layer and saved to its file. A reset drops the shown layout's user layer only and rebuilds the default plus the named layout's operations. The other layouts' files are not touched.
- **Files.** `riso.default` keeps `layout.json`. Any other named layout `<name>` has its user layer in `layouts/<name>.json` in the data directory, named by `riso_paths`. The shell builds no path: the startup gives it `set_named_layout_files`, a function from a name to its file, as it gives `set_layout_file` today. Without it, named layouts' user layers live in memory only, with a `WARN` once. A file is read when the window switches to its layout, with feature 12's rules for a missing or broken file.

## What code can do

```rust
// riso_paths (dev part `named_layout_file`)
impl Paths {
    pub fn named_layout_file(&self, name: &str) -> PathBuf;
}

// riso_shell::dev
pub struct NamedLayout { /* records operations */ }
impl NamedLayout {
    pub fn add(&mut self, node: &str, component: &str, parent: &str, slot: &str);
    pub fn add_with_props(&mut self, node: &str, component: &str, parent: &str, slot: &str, props: impl serde::Serialize);
    pub fn remove(&mut self, node: &str);
    pub fn move_node(&mut self, node: &str, parent: &str, slot: &str, index: usize);
    pub fn set_hidden(&mut self, node: &str, hidden: bool);
    pub fn set_props(&mut self, node: &str, props: impl serde::Serialize);
}
impl Extension {
    pub fn named_layout(&self, cx: &mut App, name: &str, build: impl FnOnce(&mut NamedLayout));
    pub fn show_layout(&self, window: &mut Window, cx: &mut App, name: &str);
}
pub fn shown_layout(cx: &App, window: &Window) -> SharedString;
pub fn set_named_layout_files(cx: &mut App, file: impl Fn(&str) -> PathBuf + 'static);
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A named layout is registered | A group `named-layout-register{name, extension, operations}`, with `debug` `registered`; an `ERROR` for a refusal |
| A window switches | A group `layout-switch{from, to}`, holding a `layout-restore{file}` group for the user layer (feature 12), `INFO` `skipped` for each refused operation, `view dropped` and `component-create` for the views that leave and come, and `debug` `switched {kept, dropped, created}` |
| Switching to the shown layout | `debug` `already shown` in the group |
| An unknown name | `INFO` `no named layout {name}` in the group |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_paths/named_layout_file` (new, dev) | `Paths::named_layout_file`: `layouts/<name>.json` in the data directory |
| `riso_shell/shell` dev version | `NamedLayout`, `named_layout`, `show_layout`, `shown_layout`, one user layer and one file per named layout, and the switch |
| `riso/startup` | Dev copy that uses the dev shell and gives it `set_named_layout_files` from `Paths::named_layout_file`. While `named-layouts` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **Paths, unit test:** `named_layout_file("layout_tour.swap")` is `layouts/layout_tour.swap.json` in the data directory.
- **Shell, GPUI tests, with the dev shell:**
  - a window starts with `riso.default`; switching to a named layout applies its operations over the default; switching back gives the default with its user layer;
  - a node shown before and after a switch keeps its view (the same entity) and its state; a node that leaves has its view dropped; a node that comes has its view created;
  - an operation while a named layout is shown is saved to its file only; switching away and back restores it; a reset drops only that layout's user layer;
  - a named layout's operation that the rules refuse is skipped with an `INFO`, and the rest applies;
  - the registration refusals each log an `ERROR`; an unknown name keeps the layout with an `INFO`; switching to the shown layout changes nothing;
  - a zoomed node is zoomed out by a switch.
- **Logging**, alone in its file: `layout-switch{from, to}` holds `layout-restore`, the skipped `INFO`, the views dropped and created, and `switched {kept, dropped, created}`.

## Test extension: `examples/layout_tour`

A new case, `named-layouts`. A row with cards `a`, `b`, `c` beside a column of controls: `default`, `swap`, `focus`, `hide c`, `reset layout`, and the name of the shown layout. `layout_tour.swap` moves `c` before `a`. `layout_tour.focus` hides `a` and `c`, and adds a card `d` after `b`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-named LAYOUT_TOUR=named-layouts cargo run dev` | `a`, `b`, `c`; shown: `riso.default`. Two `named-layout-register` groups. |
| Click `count` on `b` twice, then `swap` | `c`, `a`, `b`; `b` still shows `count: 2`; `layout-switch{from=riso.default, to=layout_tour.swap}` with `switched {kept=…, dropped=0, created=0}`. |
| Click `focus` | `b` and `d` only; `b` still shows `count: 2`; `d` is created, `a` and `c` hidden. |
| Click `swap`, then `hide c`, then `default` | In `swap`, `c` hides; back in `default`, `c` is shown: the hide is in `layouts/layout_tour.swap.json` only. |
| Click `swap` again, then `reset layout` | `c` is back, before `a`: only `swap`'s user layer was dropped. |

## Under review

- `riso_paths/named_layout_file`
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Several windows: whether windows that show the same named layout share its user layer is an open decision of the window engine's roadmap. This feature has one window.
- Remembering which named layout a window showed across restarts: with the window engine's sessions.
- A key or a menu to switch: the extension calls `show_layout`; a command for the palette comes with a feature that needs it.
- The user creating, renaming, or deleting named layouts.
