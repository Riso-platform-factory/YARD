# Layout feature 5: split-resize

## What it adds

The line between two children of a split becomes a handle. Dragging it changes how the split's length is shared between its children. A child never gets smaller than a minimum size. Double-clicking a handle makes the shares equal again.

## Behavior

Everything here follows the handles and resize rules of Zed's `pane_group.rs`.

- **Handle.** Each line has a handle 4px wide, centered on the line. On hover, the cursor becomes a column-resize cursor in a row, or a row-resize cursor in a column. It keeps that cursor while dragging.
- **Shares.** Every child has a share. At first the shares are equal. Dragging a handle moves the line with the mouse:
  - the child before the line grows;
  - the child after it shrinks, or the other way round.
- **Minimum size.**
  - The minimums are 80px wide in a row and 100px high in a column, as in Zed.
  - When the neighbour reaches its minimum and the drag goes on, the next child shrinks in turn. Dragging pushes the children down the line until all of them are at their minimum.
- **Reset.** A double click on a handle makes all the shares of that split equal again.
- **Resizing the window** keeps the shares. Each child keeps its fraction of the split's length.
- **Too small already.** If a split is too small to give every child its minimum, the drag changes nothing.
- **Lifetime.** The shares live as long as the window. They are not saved; saving comes with features 11 and 12.

## Logging

Dragging is high-frequency work. All lines are written by `yard_ui`, and each carries the split's ID, which is its node ID.

| When | Level | Line |
|---|---|---|
| Mouse down on a handle | `debug` | `resize started` with the handle's index |
| Each mouse move that changes the shares | `trace` | the new shares |
| Mouse up after a drag | `debug` | `resize ended` with the final shares |
| Double click | `debug` | `shares reset` |

A drag is not a group. It spans many separate events, and a group must surround work while it runs.

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/axis` | Takes an ID and the shares, which its caller keeps. Draws the handles, and handles dragging, the minimum sizes, and the reset. Adapted from `pane_group.rs`: `compute_resize`, the handle hitboxes, and the mouse events. |
| `yard_shell/shell` | A split keeps its shares and gives its node ID to the axis. |
| `yard/startup` | A dev copy that uses the dev shell, as before. |

All three parts are in production. They change through dev versions beside the production ones, as in features 3 and 4. `cargo run` stays as approved until promotion.

## Test extension: `examples/layout_tour`

It is loaded again. The cases of feature 4 are the playground: the shares are changed by hand, with the mouse.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev`, then hover the line between `a` and `b` | The cursor becomes a column-resize cursor. |
| Same, drag that line to the right | `a` grows and `b` shrinks. The cards show their new widths. `c` does not change. |
| Same, keep dragging right until `b` is 80px wide, then further | `b` stays at 80px and `c` shrinks, down to 80px. Then nothing moves. |
| Same, double-click the line | All three cards are equal again. |
| Same, resize the window after a drag | The cards keep their fractions of the width. |
| `LAYOUT_TOUR=column cargo run dev`, drag a line down | The same in height, with a row-resize cursor and a 100px minimum. |
| `LAYOUT_TOUR=nested-split cargo run dev`, drag the row's line, then the column's | Each split changes only its own children. |

In the trace, with `yard_ui` under review at `debug`: `resize started`, then `resize ended` with the shares, and `shares reset` on a double click.

## Under review

`yard_ui/axis`, `yard_shell/shell`, `yard/startup`, all in their dev versions.

## Not in this feature

- Saving the shares: features 11 and 12.
- Resizing with the keyboard.
- Collapsing a child to nothing.
- Minimum sizes set per component.
