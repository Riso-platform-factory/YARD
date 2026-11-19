# Layout feature 20: fixed-sizes

## What it adds

A child of a split is either flexible, with a share as today, or fixed, with a size in pixels. When the split grows, only the flexible children grow, so a side panel keeps its width while the window is resized, as Zed's docks keep theirs. When the split shrinks, the flexible children shrink first, down to their minimum; only then do the fixed children shrink, in proportion to their sizes, and never below the minimum.

## How it works

- **The prop.** A split has a second prop beside `shares` (feature 19): `sizes`, an object from a child's node ID to its size in pixels, a positive number. A child in `sizes` is fixed, and its entry in `shares`, if any, is not used while it is fixed. A child not in it is flexible. A value that is not a positive number is read as no entry, with feature 13's `WARN`.
- **Laying out a split of length `L`.** Let `F` be the sum of the fixed sizes of the shown children, and `M` the sum of the minimums of the shown flexible children (80 px across, 100 px down, as today).
  - If `L − F ≥ M`: each fixed child gets its size, and the flexible children share `L − F` by their shares.
  - Otherwise: each flexible child gets its minimum, and the fixed children share `L − M` in proportion to their sizes, each at least its own minimum. Their sizes in the props do not change; when the split grows again, they get them back.
  - A split whose children are all fixed gives the last shown child what is left or missing, so the split is always filled.
- **Dragging a handle** between two children:
  - two flexible children: their shares change, as today;
  - a fixed and a flexible child: the fixed child's size changes by the distance dragged, and the flexible child takes the rest;
  - two fixed children: one grows and the other shrinks by the distance dragged.

  A size never goes below the minimum. At the mouse up, one `set-props` records the new `shares` and `sizes` of the children shown, as feature 19 does.
- **A double click** on a handle, which feature 19 turns into `{"shares": null}`, leaves `sizes` alone: fixed children stay fixed.

## In the UI kit

`yard_ui`'s axis takes, for each child, a share or a fixed size: `AxisSize::Flex(share)` or `AxisSize::Fixed(pixels)`. It lays them out and drags them as above. `on_resized` now tells a drag from a double click: it reports `Resized::Dragged(sizes)` with the sizes as they are after a drag, and `Resized::Reset` after a double click, so that a drag that happens to end on equal shares is still a drag. A caller that passes only shares sees today's layout.

## What code can do

```rust
// yard_ui (production, in place)
pub enum AxisSize { Flex(f32), Fixed(Pixels) }
pub fn axis(id, direction, sizes: Rc<RefCell<Vec<AxisSize>>>, children) -> Axis;   // was Vec<f32>
impl Axis {
    pub fn on_resized(self, f: impl Fn(Resized, &mut Window, &mut App) + 'static) -> Self;   // was Vec<f32>
}
pub enum Resized { Dragged(Vec<AxisSize>), Reset }
```

The dev shell has every item of the production shell, unchanged; the change is in how its splits keep and pass their sizes.

## Logging

| Situation | Log |
|---|---|
| A split lays out with too little room for its fixed children | `trace` `fixed children shrunk {split, missing}`: layout runs on every frame |
| A drag ends with changed sizes | The `layout-op{op=props, node=<split>}` of feature 19, with `props set {keys=shares,sizes}` or only the keys that changed |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/axis` (production, in place) | `AxisSize`, layout and dragging with fixed children, `Resized` and `on_resized` with it. Stamped again. |
| `yard_shell/shell` (production, in place) | Only what the changed axis signature needs: its splits pass `AxisSize::Flex` as today. Stamped again. |
| `yard_shell/shell` dev version | Splits read `sizes` from their props and pass fixed children to the axis |
| `yard/startup` | Dev copy that uses the dev shell. While `fixed-sizes` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:**
  - a fixed child keeps its size when the axis grows and the flexible children take the growth;
  - when the axis shrinks, the flexible children shrink first to their minimum, then the fixed children shrink in proportion, never below the minimum;
  - when it grows back, the fixed children get their sizes back;
  - dragging between a fixed and a flexible child changes the fixed size; between two fixed children, one grows and the other shrinks;
  - `on_resized` reports `Resized::Dragged` with `AxisSize`s after a drag, and `Resized::Reset` after a double click;
  - a caller that passes only `Flex` sees today's layout.
- **Shell, GPUI tests, with the dev shell:**
  - a split placed with `{"sizes": {"side": 240}}` gives `side` 240 px in any window wide enough;
  - a drag of `side`'s handle records one `set-props` with its new size under `sizes`;
  - sizes are saved and restored; a reset gives the default layout's sizes;
  - a double click leaves `sizes` alone;
  - a size that is not a positive number is read as no entry, with a `WARN`.
- **Logging**, alone in its file: a drag gives one `layout-op{op=props}` with `props set` naming `sizes`.

## Test extension: `examples/layout_tour`

A new case, `fixed-sizes`. The layout is a row with `side`, a card fixed at 240 px, and `main`, a flexible card, with the controls above them in a column.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-fixed LAYOUT_TOUR=fixed-sizes cargo run dev` | `side` shows `240px` wide, `main` the rest. |
| Make the window wider, then narrower | `side` stays `240px`; only `main` changes. |
| Make the window very narrow | `main` stops at its minimum, then `side` shrinks; widen it again and `side` is `240px` again. |
| Drag the line between `side` and `main` | `side` gets the new width; one `layout-op{op=props}` naming `sizes`. Quit and start: the same width. |
| `reset layout` | `side` is `240px` again. |

## Under review

- `yard_ui/axis` and production `yard_shell/shell`, in place
- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- Sizes in rems that follow the UI font, and minimums that components declare: feature 21.
- Hiding a child by dragging it below its minimum: feature 22.
