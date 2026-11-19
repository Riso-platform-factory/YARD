# Layout feature 19: sizes-as-props

## What it adds

The shares of a split are props of the split, so a resize is an operation like the others: recorded in the user layer, saved, restored at the next start, and dropped by a reset. The steps of one drag join into one operation, made when the mouse is released. Until now, shares were view state, lost at every start.

## How it works

- **The prop.** A split (`yard.row`, `yard.column`) has one prop, `shares`: an object from a child's node ID to its share, a positive number. A child not in it has the share 1. A split placed without props has equal shares, as today.
- **Why by node ID.** A share follows its child when other children are added, removed, moved, or hidden, and a saved share finds its child at the next start, as feature 12 needed and could not do.
- **A drag.** While the mouse is down, the split's view changes its shares live, and nothing is recorded, as today. When the mouse is released, the shell runs one `set-props` on the split with the shares of the children shown, by node ID. A drag that changed nothing records nothing.
- **A double click** on a handle, which gives the split equal shares in Zed, is one `set-props` with `{"shares": null}`, so the shares go back to the default.
- **Reading.** The split reads its shares from its props whenever they change: after a `set-props` from any extension, a restore, or a reset. An entry for a node that is not a child is kept in the props and ignored by the view, so that a child moved out and back gets its share again. A value that is not a positive number is read as 1, with the `WARN` of feature 13.
- **Hidden children** keep their entries; the shown children share the length in the proportions of their entries, as today.

## In the UI kit

`yard_ui`'s axis, which draws a split's children and its handles, tells its owner when a resize ends and when the shares are reset by a double click: `on_resized(Fn(Vec<f32>, &mut Window, &mut App))`, called once at the end of a drag that changed the shares and once on a double click, with the shares of the children shown. Nothing else in the axis changes.

## What code can do

```rust
// yard_ui (production, in place)
pub fn axis(…) -> Axis;   // as today
impl Axis {
    pub fn on_resized(self, f: impl Fn(Vec<f32>, &mut Window, &mut App) + 'static) -> Self;
}
```

The dev shell has every item of the production shell, unchanged; the change is in how its splits keep their shares.

## Logging

| Situation | Log |
|---|---|
| A drag ends with changed shares, or a handle is double-clicked | The group `layout-op{op=props, node=<split>, extension=yard}`, with the model's `props set {keys=shares}` and the split's `props-read` |
| A drag ends with no change | `debug` `resize ended`, as today, and no operation |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/axis` (production, in place) | `on_resized`. Stamped again. |
| `yard_shell/shell` dev version | Splits read their shares from their `shares` prop and run `set-props` from `on_resized`; a reset and a restore give them their props |
| `yard/startup` | Dev copy that uses the dev shell. While `sizes-as-props` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a drag that moves a handle calls `on_resized` once, at the mouse up, with the new shares; a click on a handle without a move does not call it; a double click calls it with equal shares.
- **Shell, GPUI tests, with the dev shell:**
  - a drag records exactly one `set-props` on the split, with the shares by node ID;
  - a split placed with `{"shares": {"a": 3}}` gives `a` three times the length of its siblings;
  - after a child is moved out and back, it has its share again;
  - hiding a child keeps its entry, and the shown children share the length in their proportions;
  - the shares are saved and restored at the next start;
  - a reset gives equal shares again;
  - a double click records `{"shares": null}` and gives equal shares;
  - a share that is not a positive number is read as 1 with a `WARN`.
- **Logging**, alone in its file: a drag gives one `layout-op{op=props}` with `props set {keys=shares}`.

## Test extension: `examples/layout_tour`

The case `user-layer` (feature 11) is used as it is: it has a row of two tabs containers. Its controls show the user layer's length.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-sizes LAYOUT_TOUR=user-layer cargo run dev`, drag the line between `left` and `right` to the right | One `layout-op{op=props, node=…}` at the mouse up, not one per step; `user layer: 1`. |
| Quit and start again | The line where it was left; `restored {applied=1, skipped=0}`. |
| Double-click the line | Equal shares; `user layer: 2`. |
| Drag it again, then `reset layout` | Equal shares; `user layer: 0`. |

## Under review

- `yard_ui/axis`, in place
- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Fixed sizes, and the minimums of components: features 20 and 21.
- The selected tab and the zoom: they stay view state, not saved.
