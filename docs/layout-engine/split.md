# Layout feature 4: split

## What it adds

The first container. A split shows the nodes of its slot side by side, in a row or in a column, each in an equal share, with a line between them. A split can sit in another split's slot, so splits nest.

## Model

- The shell itself provides two component types, `yard.row` and `yard.column`. Each has one slot, `children`.
- They belong to the namespace `yard`, which is the shell's own:
  - Any extension may place nodes of a `yard` component. This is the one exception to feature 3's rule that an extension places only components of its own namespace.
  - No extension may register a component in `yard`, and the startup refuses an extension named `yard`.
- A split with no children shows nothing. A split with one child shows it filling the split.

## How it is drawn

The layout follows the axis layout of Zed's `pane_group.rs`, the element that places panes side by side.

- Each child is laid out at an exact size: the split's length along its axis, divided by the number of children, rounded to whole pixels.
- A line of the theme's `border` color is drawn between two neighbours.
- There is no handle to drag yet. That is feature 5.

Zed's code is GPL. It is copied and adapted into `yard_ui` only, as a dev part `yard_ui/axis`, keeping Zed's copyright and naming the file it comes from.
- `yard_ui/axis` takes an axis and a list of elements, and draws them as above.
- It knows nothing of nodes or slots.
- The split component in `yard_shell` uses it.

## Logging

- The split is created like any component: in `component-create`, nested in the tree.
- Inside it, a `debug` line gives the axis and the number of children.
- Laying out the children happens on every frame, so it logs only at `trace`: the size given to each child.

## Where the code goes

| Part | What changes |
|---|---|
| `yard_ui/axis`, dev, new | The axis element, adapted from `pane_group.rs` |
| `yard_shell/shell` | A dev version beside the production one, as in feature 3. The shell registers `yard.row` and `yard.column` itself, lets any extension place them, and refuses an extension named `yard` |
| `yard/startup` | A dev copy that uses the dev shell, as in feature 3 |

`cargo run` stays as approved until promotion.

## Test extension: `examples/layout_tour`

It is loaded again, with the guest.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev` | Three cards, `a`, `b`, `c`, side by side, with a line between them. Each card's width is a third of the window, give or take a pixel. |
| `LAYOUT_TOUR=column cargo run dev` | The same three cards, stacked. |
| `LAYOUT_TOUR=nested-split cargo run dev` | A row: `a` on the left, and on the right a column with `b` above `c`. `a` is half the width; `b` and `c` are each half the height. |
| `LAYOUT_TOUR=one cargo run dev` | A row with only `a`, which fills the window. |
| `LAYOUT_TOUR=empty-split cargo run dev` | A row with no children. The window shows only the background. |
| `LAYOUT_TOUR=row LAYOUT_GUEST=split cargo run dev` | The guest places its card as a fourth child of the host's row, `root`. There are four equal shares, and the guest's card is the last. |
| `LAYOUT_TOUR=reserved cargo run dev` | The tour registers `yard.badge`. It is refused with an `ERROR`, because `yard` is the shell's namespace. |

Resizing the window keeps the shares equal. The cards show their sizes as they change.

## Under review

- `yard_ui/axis`
- `yard_shell/shell` (dev version)
- `yard/startup` (dev copy)

## Not in this feature

- Dragging the line between two children to change their shares, and a minimum size: feature 5.
- Shares other than equal ones.
- Gaps, padding, or alignment other than filling.
