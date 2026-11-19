# Layout feature 22: snap-hide

## What it adds

Dragging a split's line far past a child's minimum hides that child, as a `hide` operation: a side panel dragged shut disappears. A hidden child of a split leaves a thin line at its place, and dragging that line out shows the child again, as a `show` operation. Until now, a drag stopped at the minimum, and a hidden child could only come back through an action.

## How it works

- **Snapping shut.** While a handle is dragged, a child beside it that would be made smaller than **half its minimum** is shown collapsed, and the drag goes on as if it were not there. At the mouse up, the child is hidden with one `hide` operation, through the same path as any `hide`, so its rules hold: a required node (feature 15) is never snapped shut; the drag stops at its minimum as before.
- **Changing one's mind.** Dragging back before the mouse up, past half the minimum, brings the child back at its minimum, and nothing is recorded for the snap.
- **What is kept.** The hidden child keeps its entry in the split's `shares` or `sizes` (features 19 and 20), so it comes back at its size. The drag records no `set-props` for it. The other child of the drag gets the room, and its share or size is recorded as feature 19 says, so one snap is at most a `set-props` and a `hide`.
- **The collapsed line.** Each hidden child of a split is drawn as a line at the place where it would be, between its shown neighbours or at the split's edge. Like the split's handles, it takes no room: a hidden child still gives all its space to its siblings, as feature 7 says, and the line is only drawn and grabbed over the boundary. At the split's outer edge, which is often the window's edge where the system resizes the window, the line is drawn one pixel inside, and its hit area lies entirely inside the split and is twice a handle's, so that it can be seen and grabbed. It uses the handle's cursor and its drop target is off. This holds for a child hidden in any way, by a snap or an action, so that every hidden child of a split can be brought back by the mouse.
- **Snapping open.** Dragging a collapsed line by more than half the child's minimum shows the child, as one `show` operation at the mouse up; during the drag it grows from its minimum with the pointer, as a normal resize, and its size at the mouse up is recorded with feature 19's `set-props`. A drag shorter than that leaves it hidden and records nothing.
- **Nothing else changes for hidden children**: a tabs container still drops their tabs, and a hidden child of a container that is not a split is not drawn at all.

## In the UI kit

`yard_ui`'s axis knows which children are collapsed: `AxisChild` gains `collapsed: bool`. A collapsed child takes no room, its line drags it open, and the axis tells its owner what a drag did: `Resized::Dragged { sizes, snapped_shut: Vec<usize>, snapped_open: Vec<usize> }` by the children's indices, beside `Resized::Reset`. Snapping is the owner's choice: `Axis::can_collapse(f)` says which children may snap shut; without it no child does, and a drag stops at the minimum as before.

## What code can do

```rust
// yard_ui (production, in place)
pub struct AxisChild { pub size: AxisSize, pub min: Pixels, pub collapsed: bool }
pub enum Resized { Dragged { sizes: Vec<AxisSize>, snapped_shut: Vec<usize>, snapped_open: Vec<usize> }, Reset }
```

The dev shell has every item of the production shell, unchanged; the change is in how its splits draw hidden children and turn snaps into operations.

## Logging

| Situation | Log |
|---|---|
| A child is snapped shut or open at the mouse up | The `layout-op{op=hide, node}` or `layout-op{op=show, node}` of feature 8, with `extension=yard`, and the `set-props` of feature 19 if a size changed |
| A child passes half its minimum during a drag | `debug` `snap preview {split, child, shut}`, once each time it crosses |
| A required child would be snapped | `debug` `required child kept {split, child}`, and the drag stops at its minimum |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/axis` (production, in place) | `collapsed`, collapsed lines, snapping, and `Resized::Dragged` with the snapped indices. Stamped again. |
| `yard_shell/shell` (production, in place) | Only what the changed signature needs: production splits pass `collapsed: false` and do not show hidden children, as today. Stamped again. |
| `yard_shell/shell` dev version | Splits pass hidden children as collapsed, and turn snaps into `hide` and `show` operations |
| `yard/startup` | Dev copy that uses the dev shell. While `snap-hide` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a drag past half a child's minimum collapses it and reports `snapped_shut`; dragging back before the mouse up does not; a collapsed child's line, dragged out past half its minimum, reports `snapped_open`; a shorter drag does not.
- **Shell, GPUI tests, with the dev shell:**
  - a side card dragged shut is hidden with one `hide` operation, and its entry in `sizes` is kept;
  - its collapsed line, dragged out, shows it with one `show` operation, at its kept size or the size dragged to;
  - a required node is never snapped shut;
  - a child hidden by `set_hidden` also shows a collapsed line that brings it back;
  - the snaps are saved and restored with the user layer, and a reset shows the child again.
- **Logging**, alone in its file: a snap shut gives `layout-op{op=hide, extension=yard}`; `snap preview` once per crossing.

## Test extension: `examples/layout_tour`

The case `fixed-sizes` of feature 20 is used as it is: `side` is a fixed card beside `main`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-snap LAYOUT_TOUR=fixed-sizes cargo run dev`, drag the line between `side` and `main` far to the left | Past half of `side`'s minimum, `side` collapses; at the mouse up, `layout-op{op=hide, node=side}`. A thin line stays at the left edge. |
| Drag that line to the right | `side` comes back; `layout-op{op=show, node=side}`. |
| Drag it shut, then back before releasing | Nothing is recorded. |
| Drag it shut, quit, start again | `side` is still hidden, with its line; drag it out and it is back at 240 px. |
| `reset layout` | `side` shown at 240 px. |

## Under review

- `yard_ui/axis` and production `yard_shell/shell`, in place
- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Snapping a tabs container's children: tabs have no lines.
- An animation of the collapse.
