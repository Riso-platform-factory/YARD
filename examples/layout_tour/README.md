# layout_tour

A test extension for the layout engine. `app/riso/src/dev/review.rs` lists it among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, so `cargo run dev` loads it into the running app. When every feature it covers is in production, it is taken out of both until the next feature is under review.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does: an empty window in the theme's background. With a case, it registers components and places nodes through `riso_shell`, as any extension would.

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

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
