# layout_tour

A test extension for the layout engine. When `app/riso/src/dev/review.rs` lists it as `("layout_tour", layout_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers so far is in production, so it is not loaded now; add both back to check those features again, or when the next feature is under review.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does: an empty window in the theme's background. With a case, it registers components and places nodes through the handle the startup gives it, as any extension would.

It grows with the layout engine, one case per feature. Its components are test cards: a card shows its node ID and its own size, and stands for no feature.

It has no stage and is never in production.

## Cases

The case is chosen with the environment variable `LAYOUT_TOUR`. An unknown case logs a WARN that names the known ones. Every case runs inside a `layout-tour` group.

### Feature 1: component-registry

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=card cargo run dev` | One card fills the window and shows `root` and its size. Resizing the window changes the size. In the trace: `component-register`, then `root-place`, then `component-create` inside `window-open`. |
| `LAYOUT_TOUR=missing cargo run dev` | The root names `layout_tour.missing`, which is not registered. The window shows only the background. `component-create` fails (✗) with an ERROR inside `window-open`. |
| `LAYOUT_TOUR=twice cargo run dev` | Two roots are placed. The second `root-place` is refused with an INFO and still succeeds (✓), and the first card is shown. |
| `LAYOUT_TOUR=bad-id cargo run dev` | The component ID `Layout Tour.Card` breaks the slug rule. `component-register` and `root-place` each fail (✗) with an ERROR, and the window is empty. |

### Feature 2: slots

The card has one slot, `body`. It draws a border, its ID and size at the top, and the nodes of its `body` under them in a column, each taking an equal share.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=nested cargo run dev` | `root` holds `left` and `right`; `left` holds `inner`. Each card shows its own size. In the trace, the `component-create` groups nest as the cards do. |
| `LAYOUT_TOUR=duplicate-id cargo run dev` | The second `node-place` of `left` is refused with an INFO (`the node ID is already in the tree`) and succeeds (✓). One `left` is shown. |
| `LAYOUT_TOUR=missing-parent cargo run dev` | Placing `lost` in `nowhere.body` is refused with an INFO (`the parent is not in the tree`). Only `root` is shown. |
| `LAYOUT_TOUR=missing-slot cargo run dev` | `aside` is placed in `root.side`, which the card does not declare. Its `component-create` fails (✗) with an ERROR inside `window-open`. Only `root` is shown. |
| `LAYOUT_TOUR=missing-child cargo run dev` | `left` names an unregistered component and holds `inner`. `left`'s `component-create` fails (✗) and `inner` is never created; `right` is shown. |
| `LAYOUT_TOUR=duplicate-slot cargo run dev` | `layout_tour.double` declares `body` twice. Its `component-register` fails (✗) with an ERROR. The `root` card is shown. |

An ERROR fails its group and every group around it, up to `app-startup`.

### Feature 3: extension-contribution

`layout_tour` is the host; `examples/layout_guest` is the guest, and its README gives the cases where both take part.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=host cargo run dev` | The `root` card with an empty `body`. Each extension runs inside its own `extension-init{extension=…}` group, and `component-create` shows `extension=layout_tour`. |

### Feature 4: split

`riso.row` and `riso.column` are the shell's own components, with one slot, `children`. Any extension may place them.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev` | Cards `a`, `b`, `c` side by side, each a third of the width, with a line between them. In the trace: `split created {axis=row, children=3}` inside the row's `component-create`. |
| `LAYOUT_TOUR=column cargo run dev` | The same cards stacked, each a third of the height. |
| `LAYOUT_TOUR=nested-split cargo run dev` | `a` on the left half; on the right half, `b` above `c`. The column's `component-create` is nested in the row's. |
| `LAYOUT_TOUR=one cargo run dev` | A row with only `a`, which fills the window. |
| `LAYOUT_TOUR=empty-split cargo run dev` | A row with no children: only the background, and `split created {axis=row, children=0}`. |
| `LAYOUT_TOUR=row LAYOUT_GUEST=split cargo run dev` | The guest's card is a fourth, equal child of the host's row. |
| `LAYOUT_TOUR=reserved cargo run dev` | Registering `riso.badge` is refused with an ERROR: `riso` is the shell's namespace. |

Resizing the window keeps the shares equal; each card's size catches up a frame after the resize.

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
