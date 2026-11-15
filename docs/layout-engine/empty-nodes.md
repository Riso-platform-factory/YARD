# Layout feature 23: empty-nodes

## What it adds

A component can say that it is empty now, such as a panel with nothing to show or an editor area with no open file. A container gives an empty child no room, as if it were hidden, and is itself empty when nothing in its slots is shown and not empty, so a column whose children are all empty disappears too. Emptiness is the component's state, not the user's choice: it is not an operation, it is not saved, and the node comes back as soon as its component says it is no longer empty.

This is what Zed's docks do when they have no panel to show, and what its pane group does when a pane has no item: the space goes to the others.

## How it works

- **Saying it.** A component's view implements `Emptiable`, with one method, `is_empty(&self, cx: &App) -> bool`. The component says so once, with `Extension::register_empty::<V>(cx, component)`, as feature 14's titles are declared. It is refused with an `ERROR`, a bug of the extension, for an invalid ID, a component outside the extension's namespace, or a second declaration. It can be called before or after the component is registered. A component that does not say so is never empty.
- **When it changes.** The view changes its state and notifies, and the next frame lays the window out again. Nothing is recorded.
- **Containers:**
  - a split gives an empty child no room and no handle, and its other children share the length as if that child were hidden; an empty child keeps its entry in `shares` and `sizes`, and gets its size back when it is no longer empty;
  - a tabs container drops an empty child's tab, and selects its first shown child when the selected one becomes empty, as it does for a hidden child;
  - a container (`riso.row`, `riso.column`, `riso.tabs`) is empty when at least one of its children is empty and every other one is empty or hidden; a container whose children are all hidden and none empty is not empty, so that feature 22's collapsed lines still bring them back; a container with no children is not empty; an empty container is treated by its own parent as above, all the way up;
  - the window's root and the shell's frame (title bar, body, status bar) are never treated as empty.
- **Not a hidden node.** An empty node is not hidden: `is_hidden` is false, the `riso::Hide` action and the operations of feature 8 see it as shown, and it has no collapsed line of feature 22, since dragging it out would show nothing. A node that is both hidden and empty stays hidden when it stops being empty.
- **Focus.** If the focused node becomes empty, the focus moves as it does when a focused node is hidden.

## What code can do

```rust
// riso_shell::dev
pub trait Emptiable: gpui::Render {
    fn is_empty(&self, cx: &App) -> bool;
}
impl Extension {
    pub fn register_empty<V: Emptiable + 'static>(&self, cx: &mut App, component: &str);
}
pub fn is_empty(cx: &App, node: &str) -> bool;   // a node, or a container whose children are all empty
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A component declares that it can be empty | A group `empty-register{component}`, with `debug` `can be empty`; an `ERROR` for a refusal |
| A node's emptiness is read when its container lays out | `trace` `empty {node, empty}`: layout runs on every frame |
| A node becomes empty or stops being empty | `debug` `emptiness changed {node, empty}`, once per change |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | `Emptiable`, `register_empty`, `is_empty`, and containers that give empty children no room |
| `riso/startup` | Dev copy that uses the dev shell. While `empty-nodes` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- GPUI tests, with the dev shell:
  - in a column of two cards, one empty, the other takes the whole height; when the first stops being empty, both share it again in their old proportions;
  - a split whose children are all empty is itself empty, and its parent gives it no room;
  - a tabs container drops an empty child's tab and selects its first shown child;
  - an empty node is not hidden, has no collapsed line, and records no operation;
  - a hidden node that stops being empty stays hidden;
  - `register_empty` before and after `register`; its refusals each with an `ERROR`.
- **Logging**, alone in its file: `empty-register{component}` with `can be empty`; `emptiness changed` once per change.

## Test extension: `examples/layout_tour`

A new case, `empty-nodes`. Its component `layout_tour.list` shows a number of items, and is empty when it has none; buttons add and remove items. The layout is the controls above a row with `left`, a column of two lists, `top` (two items) and `bottom` (none), and `right`, a plain card.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-empty LAYOUT_TOUR=empty-nodes cargo run dev` | `left` shows only `top`, at the column's full height; `bottom` is empty and takes no room. |
| Click `add to bottom` | `bottom` appears under `top`, and they share the column; `emptiness changed {node=bottom, empty=false}`. |
| Click `clear top`, then `clear bottom` | `top` disappears, then `bottom`, and then the whole `left` column: `right` takes the row. |
| Click `add to top` | `left` comes back with `top`. |
| `user layer: 0` all along | Nothing was recorded. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Containers that extensions draw themselves: they show an empty child as any other, and are empty only if their component says so. Their slots do not read emptiness yet.

- An empty state drawn in place of the content, such as Zed's "no open files" page: a component draws that itself when it chooses not to say it is empty.
