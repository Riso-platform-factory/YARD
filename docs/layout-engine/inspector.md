# Layout feature 31: inspector

## What it adds

A test extension that shows the layout tree beside any case of the layout tour: each node with its component, its extension, its props, whether it is hidden, and what put it there (the default layout, a named layout, the user layer, or an item slot). It follows the tree as it changes, so a drag, a hide, a reset, or a switch of named layout shows at once. The shell gains a read-only view of its tree for it.

## How it works

- **What the shell gives.** `riso_shell::inspect(cx)` returns the window's tree as a list of `NodeInfo`, depth first, in each node's slots' order and then each slot's children's order, as the tree holds them (shown or hidden), with the copies of an item slot (feature 28) in their slot. `NodeInfo` holds:
  - `id`, `depth` (the root is 0), `slot` (the parent's slot, none for the root);
  - `component`, `extension`, `props` (the node's props as JSON, `null` when it has none);
  - `hidden` (its own hidden flag; for a copy, its template's);
  - `origin`: `Default` (placed in the default layout), `NamedLayout(name)` (added by the shown named layout's operations, feature 30), `UserLayer` (added by an operation of the user layer), or `Copy(key)` (a copy of an item slot).
- **Following changes.** `riso_shell::observe_layout(cx, f)` calls `f` after the tree changes: an operation applied, a reset, a restore, a switch of named layout, and items that change an item slot's copies. It is called once per change, after the change is complete, never during a render. A change of view state only (a selected tab, a zoom, sizes being dragged, focus) does not call it. The returned `Subscription` stops it when dropped.
- **Where the inspector stands.** The body holds one root. With `LAYOUT_INSPECTOR` set, the layout tour places a `riso.row` root `tour_frame`, with the props `{"sizes": {"inspector": 360}}` (feature 20), and places each case's root in it instead of as the root, with the same ID. The extension `layout_inspector`, loaded after `layout_tour`, places its node `inspector`, of its component `layout_inspector.panel`, in `tour_frame`'s slot `children`, last (feature 3: a node in another extension's slot), so it keeps a fixed width of 360 px. Without `LAYOUT_INSPECTOR`, the tour places its roots as today and the inspector places nothing.
- **Its look**, as Zed's syntax tree view (`crates/language_tools/src/syntax_tree_view.rs`, `render_node`): a `List` of the UI kit with a `ListHeader` `Layout` and the number of nodes, then one `ListItem` per node, indented by its depth (`indent_level`), in a vertical scroll:
  - its start shows the node ID, then its component and its props as one line, muted and truncated;
  - its end shows its extension and its origin (`default`, the named layout's name, `user`, or `copy <key>`), muted, small;
  - a hidden node's row is drawn disabled.
- **Refreshing.** The panel holds the last `inspect` and replaces it when `observe_layout` calls it, then notifies. It reads nothing on a frame.

## What code can do

```rust
// riso_shell::dev
pub struct NodeInfo {
    pub id: SharedString,
    pub depth: usize,
    pub slot: Option<SharedString>,
    pub component: SharedString,
    pub extension: SharedString,
    pub props: serde_json::Value,
    pub hidden: bool,
    pub origin: Origin,
}
pub enum Origin { Default, NamedLayout(SharedString), UserLayer, Copy(SharedString) }
pub fn inspect(cx: &App) -> Vec<NodeInfo>;
pub fn observe_layout(cx: &mut App, f: impl Fn(&mut App) + 'static) -> Subscription;
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| `inspect` is called | `trace` `layout inspected {nodes}` |
| The observers are called after a change | `debug` `layout observers {count}` in the change's group |
| The inspector refreshes | A group `inspector-refresh{nodes}` in `layout_inspector` |
| The inspector cannot place itself (no `tour_frame`) | `WARN` once, in `layout_inspector`'s init |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `NodeInfo`, `Origin`, `inspect`, `observe_layout` |
| `riso/startup` | Dev copy that uses the dev shell. While `inspector` is under review, `cargo run dev` loads `review`, `layout_tour`, and `layout_inspector` on it, and not the command palette, which is built on the production shell's extension handle |
| `examples/layout_inspector` (new example crate, no stage) | The extension `layout_inspector` and its panel |
| `examples/layout_tour` (no stage) | `tour_frame` under `LAYOUT_INSPECTOR` |

## Tests

- **Shell, GPUI tests, with the dev shell:**
  - `inspect` lists a nested tree depth first, in slot and child order, with depth, slot, component, extension, props, and hidden;
  - origins: a placed node is `Default`, a node added by the shown named layout is `NamedLayout`, a node added by the user is `UserLayer`, an item slot's copy is `Copy(key)`;
  - `observe_layout` is called once after an operation, a reset, a restore, a switch, and an items change, and not after selecting a tab, zooming, or dragging a split's handle; dropping the subscription stops it.
- **Inspector, GPUI tests:** with `tour_frame`, the panel lists one row per node of `inspect`, indented by depth; after a hide, the row is disabled on the next draw, without a frame reading the tree.
- **Logging**, alone in its file: `layout observers {count}` in an operation's group; `inspector-refresh{nodes}` once per change.

## Test extension: `examples/layout_tour` and `examples/layout_inspector`

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-inspect LAYOUT_INSPECTOR=1 LAYOUT_TOUR=user-layer cargo run dev` | The case on the left, the inspector on the right: `Layout`, then `tour_frame`, `root` and the case's nodes under it (`left` with `a`, `b`; `right` with `c`, `d`; the controls), then `inspector`, indented; each `default`. |
| Drag tab `b` onto the top edge of `right`'s content (feature 26) | Rows `split-1` and `tabs-1` appear, `user`; `b` is under `tabs-1`. |
| Click `hide d` | `d`'s row is disabled. |
| Click `reset layout` | The rows are as at the start. |
| `LAYOUT_INSPECTOR=1 LAYOUT_TOUR=item-slots cargo run dev`, click `add dee` | A row `name@dee`, `copy dee`. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Picking a node in the window to find its row, or a row to show its node, as GPUI's own inspector does: when a feature needs it.
- Editing the tree or props from the inspector.
- The inspector in the product: it is a test extension, never released.
