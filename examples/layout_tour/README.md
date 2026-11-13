# layout_tour

A test extension for the layout engine. When `app/riso/src/dev/review.rs` lists it as `("layout_tour", layout_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app.

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

### Feature 5: split-resize

No new case: the splits of feature 4 are resized by hand. In the trace, with `riso_ui` at `debug`, each drag logs `resize started {id, handle}` and `resize ended {id, shares}`, where `id` is the split's node ID.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev`, hover a line | A column-resize cursor. |
| Drag the line between `a` and `b` to the right | `a` grows, `b` shrinks, `c` does not change. |
| Keep dragging right | `b` stops at 80px, then `c` shrinks to 80px, then nothing moves. |
| Double-click the line | `shares reset`; the three cards are equal again. |
| Resize the window after a drag | Each card keeps its fraction of the width. |
| `LAYOUT_TOUR=column cargo run dev`, drag a line down | The same in height, with a row-resize cursor and a 100px minimum. |
| `LAYOUT_TOUR=nested-split cargo run dev`, drag the row's line, then the column's | Each logs its own `id`, `root` or `right`, and changes only its own children. |

### Feature 6: tabs

`riso.tabs` is the shell's own component, with one slot, `children`, shown one at a time under a tab bar drawn with Zed's `Tab` and `TabBar`. Each tab's label is its child's node ID.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=tabs cargo run dev` | Tabs `a`, `b`, `c`, with `a` selected and card `a` under them. `tabs created {tabs=3}`. |
| Same, click `b`, then `c`, then `a`, then `a` again | Each click shows its card and logs `tab selected {tabs=root, tab=…}`; clicking the selected tab logs nothing. |
| `LAYOUT_TOUR=tabs-in-split cargo run dev` | Card `a` on the left, tabs `b` and `c` on the right. Dragging the row's line resizes the tabs container; clicking `c` logs `tab selected {tabs=right, tab=c}`. |
| `LAYOUT_TOUR=empty-tabs cargo run dev` | An empty tab bar over the background, `tabs created {tabs=0}`. |
| `LAYOUT_TOUR=tabs LAYOUT_GUEST=tab cargo run dev` | A fourth tab, `guest`, at the end. |

### Feature 7: hide-show

The card has a counter: clicking `count: N` adds one. `layout_tour.controls` shows `hide <id>` or `show <id>` for each node it controls; clicking it hides or shows that node. Each change runs in `layout-op{op=hide|show, node, extension}`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=hide-show cargo run dev` | A row: `controls`, `a`, `b`, a third each. |
| Same, drag the line between `a` and `b` to the right, click `b`'s count three times, then `hide b` | `b` goes away, and `controls` and `a` share the row in the proportions of their own shares. `node hidden {node=b}`. |
| Same, then `show b` | `b` is back with the width it had and `count: 3`. |
| `LAYOUT_TOUR=hide-tab cargo run dev` | `controls` on the left, tabs `a` and `b` on the right. Select `b`, count to 2, `hide b`: tab `b` goes away and `a` is shown, without a `tab selected` line. `show b`: tab `b` is back, `a` stays selected; clicking `b` shows `count: 2`. |
| `LAYOUT_TOUR=hide-root cargo run dev` | Hiding `root` is refused with an INFO, `the root cannot be hidden or shown`. |

### Feature 8: operations

`layout_tour.controls` shows the buttons of its case; each calls one operation through the extension's handle. Each operation runs in `layout-op{op, node, extension}`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=operations cargo run dev` | A row: `controls`, card `a`, and a column `right` with cards `b` and `c`. |
| Click `b`'s count twice, then `add d` | Card `d` is the last child of the row; `component-create{node=d}` sits inside `layout-op{op=add, node=d}`. |
| `move b to root` | `b` leaves the column and is the last child of the row, still at `count: 2`. |
| `remove a` | `a` goes away; `node removed {removed=1}` and `view dropped {node=a}`. |
| `move right into c` | Refused with an INFO: moving a node into its own subtree would make a cycle. |
| `move c to b.side` | Refused with an INFO: the card does not declare `side`. |
| `remove root` | Refused with an INFO. |
| `add d` again | Refused with an INFO: `d` is already in the tree. |

### Feature 9: drag-to-move

Any tab can be dragged. A drop moves its node as the shell, in `layout-op{op=move, node, extension=riso}`. With `riso_shell` at `debug`, a drag logs `drag started` and a drop that moves nothing logs `drop ignored`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=two-tabs cargo run dev` | A row of two tabs containers: `left` with `a`, `b`; `right` with `c`, `d`. |
| Count `a` to 2, drag tab `a` onto the empty part of `right`'s bar | The bar is highlighted while over it; `right` becomes `c`, `d`, `a`, and `a` keeps `count: 2`. |
| Drag tab `c` onto the right edge of `left` | The right half of `left` is highlighted; the row becomes `left`, `c`, `right`, each a third. |
| Drag a tab onto the middle of a card | No highlight; `drop ignored`. |
| Drag a tab and drop it on its own place | Nothing moves; `drop ignored`. |

### Feature 10: zoom

The card has a line `zoom`, or `zoom out` while it is zoomed. Zooming runs in `zoom{node, extension}`, zooming out in `zoom-out{extension}`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=zoom cargo run dev` | A row: card `a`, and tabs `right` with `b` and `c`. |
| Count `a` to 2, drag the row's line, select tab `c`, click `zoom` on `c` | `c` fills the window; `zoomed {node=c}`. |
| Click `zoom out` | The row is back: the line where it was, `c` selected, `a` at `count: 2`. |
| `LAYOUT_TOUR=zoom-root cargo run dev` | Zooming `root` is refused with an INFO, `the root already fills the window`. |

After a zoom, wait for the window to redraw before clicking `zoom out`: the card is laid out again at its new size first.

### Feature 11: user-layer

The controls show `user layer: N`, the number of operations applied since the window opened, and a `reset layout` button. A reset runs in `layout-reset{extension}`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=user-layer cargo run dev` | `controls`, then tabs `left` (`a`, `b`) and `right` (`c`, `d`); `user layer: 0`. |
| Select `b` and count it to 2, drag tab `a` onto `right`'s bar, drag the row's line, `hide d` | `user layer: 2`: `recorded {user_layer=1}` after the move, `{user_layer=2}` after the hide. The resize and the tab selection are not recorded. |
| `reset layout` | `left` has `a`, `b` and `right` has `c`, `d` again, in equal shares, with the first tabs selected; `b` keeps `count: 2`; `user layer dropped {dropped=2}`. |
| `reset layout` again | `nothing to reset`. |

### Feature 12: save-restore

The user layer is saved to `data/layout.json` after every recorded operation and every reset, and applied again at the next start. Use a fresh home, so that no real file is touched: `RISO_HOME=/tmp/riso-layout`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-layout LAYOUT_TOUR=user-layer cargo run dev`, drag tab `a` onto `right`'s bar, `hide d`, quit | `layout-save` and `file-write` inside each `layout-op`; `layout.json` holds a `move` (no `index`: it went last) and a `hide`. |
| The same command again | `layout-restore` ends with `restored {applied=2, skipped=0}`; `right` shows `c`, `a`, and `d` is hidden; `user layer: 2`. |
| `RISO_HOME=/tmp/riso-layout LAYOUT_TOUR=tabs cargo run dev` | The `tabs` layout has no `right` and no `d`: both operations are `skipped` with INFO, and the default `tabs` layout is shown. |
| `reset layout`, quit, start `user-layer` again | `layout.json` holds no operation; the default layout. |
| Write `not json` into `layout.json`, start `user-layer` | A WARN, `the saved layout cannot be read`, and the default layout; the file is left as it is. |

## Keymap feature 1: focused-node

The card shows `focused` when its node has the keyboard focus, and `contains focus` when a node inside it has it. Use a fresh home: `RISO_HOME=/tmp/riso-layout`.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev` | No card says `focused`. |
| Click card `b`, then card `c` | `b` says `focused`, then `c` does and `b` no longer does. `focus-change{node=b}`, then `focus-change{node=c}`. A second click on `c` logs nothing. |
| `LAYOUT_TOUR=nested-split cargo run dev`, click `c` | `c` says `focused`, and `right` contains the focus. |
| `LAYOUT_TOUR=tabs cargo run dev`, click card `a`, then the tab `b` | `focus-change{node=a}`, then `focus-change{node=root}`: the click on the tab bar focuses the tabs container. |

## Keymap feature 2: layout-actions

`LAYOUT_TOUR=actions`: a row with a `controls` card, card `a`, and tabs `right` with `b`, `c`, and `d`. The `controls` card's buttons send the shell's actions to the focused node without taking the focus themselves.

| Action | What to look for |
|---|---|
| Click `next tab` first | `layout-action{action=SelectNextTab, node=none}` refused: `no node has the focus`. |
| Click card `b`, then `next tab` three times | `c`, `d`, then `b` again; the tabs container `right` takes the focus. `previous tab`: `d`. |
| Click card `a`, then `next tab` | Refused: `no tabs container holds it`. |
| Click card `a`, then `hide` | `a` is hidden, inside `layout-action{action=Hide, node=a}`, and `focus-change{node=none}`. |
| Click card `b`, then `zoom` | `b` fills the window, inside `layout-action{action=ToggleZoom, node=b}`. |

## Keymap feature 3: default-keys

The case `actions` with keys instead of its buttons:

| Action | What to look for |
|---|---|
| `LAYOUT_TOUR=actions cargo run dev` | `keys-bind{count=5}` in `app-init`, with a `bound` line per key. |
| Press `shift-escape` before clicking anything | Refused: `no node has the focus`. |
| Click card `b`, then press `cmd-}` twice and `cmd-{` once | `c`, `d`, then `c`. `alt-cmd-right` and `alt-cmd-left` do the same. |
| Click card `a`, then press `shift-escape` twice | `a` fills the window, then comes back. |

## Keymap feature 4: focus-by-direction

`LAYOUT_TOUR=nested-split`: `a` on the left, `b` above `c` on the right. Click a card away from its `zoom` line, which zooms it.

| Action | What to look for |
|---|---|
| Click `b`, then `cmd-k cmd-down` | `c` says `focused`. `layout-action{action=FocusDown, node=b}` with `done {target=c}`. |
| Then `cmd-k cmd-left` | `a`. |
| Then `cmd-k cmd-left` again | Refused: `no node on the left`, with `a`'s bounds and the probe point. |
| Click `a`, then `cmd-k cmd-right` | `b` or `c`, whichever holds the middle of `a`'s right edge. |

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
