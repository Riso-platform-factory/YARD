# layout_tour

A test extension for the layout engine. When `app/yard/src/dev/review.rs` lists it as `("layout_tour", layout_tour::init)` among the extensions it passes to the startup, and `app/yard/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the layout engine again, or when the next feature that it shows is under review.

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

`yard.row` and `yard.column` are the shell's own components, with one slot, `children`. Any extension may place them.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=row cargo run dev` | Cards `a`, `b`, `c` side by side, each a third of the width, with a line between them. In the trace: `split created {axis=row, children=3}` inside the row's `component-create`. |
| `LAYOUT_TOUR=column cargo run dev` | The same cards stacked, each a third of the height. |
| `LAYOUT_TOUR=nested-split cargo run dev` | `a` on the left half; on the right half, `b` above `c`. The column's `component-create` is nested in the row's. |
| `LAYOUT_TOUR=one cargo run dev` | A row with only `a`, which fills the window. |
| `LAYOUT_TOUR=empty-split cargo run dev` | A row with no children: only the background, and `split created {axis=row, children=0}`. |
| `LAYOUT_TOUR=row LAYOUT_GUEST=split cargo run dev` | The guest's card is a fourth, equal child of the host's row. |
| `LAYOUT_TOUR=reserved cargo run dev` | Registering `yard.badge` is refused with an ERROR: `yard` is the shell's namespace. |

Resizing the window keeps the shares equal; each card's size catches up a frame after the resize.

### Feature 5: split-resize

No new case: the splits of feature 4 are resized by hand. In the trace, with `yard_ui` at `debug`, each drag logs `resize started {id, handle}` and `resize ended {id, shares}`, where `id` is the split's node ID.

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

`yard.tabs` is the shell's own component, with one slot, `children`, shown one at a time under a tab bar drawn with Zed's `Tab` and `TabBar`. Each tab's label is its child's node ID.

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

Any tab can be dragged. A drop moves its node as the shell, in `layout-op{op=move, node, extension=yard}`. With `yard_shell` at `debug`, a drag logs `drag started` and a drop that moves nothing logs `drop ignored`.

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

The user layer is saved to `data/layout.json` after every recorded operation and every reset, and applied again at the next start. Use a fresh home, so that no real file is touched: `YARD_HOME=/tmp/yard-layout`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-layout LAYOUT_TOUR=user-layer cargo run dev`, drag tab `a` onto `right`'s bar, `hide d`, quit | `layout-save` and `file-write` inside each `layout-op`; `layout.json` holds a `move` (no `index`: it went last) and a `hide`. |
| The same command again | `layout-restore` ends with `restored {applied=2, skipped=0}`; `right` shows `c`, `a`, and `d` is hidden; `user layer: 2`. |
| `YARD_HOME=/tmp/yard-layout LAYOUT_TOUR=tabs cargo run dev` | The `tabs` layout has no `right` and no `d`: both operations are `skipped` with INFO, and the default `tabs` layout is shown. |
| `reset layout`, quit, start `user-layer` again | `layout.json` holds no operation; the default layout. |
| Write `not json` into `layout.json`, start `user-layer` | A WARN, `the saved layout cannot be read`, and the default layout; the file is left as it is. |

### Feature 13: props

The card `layout_tour.tinted` takes props `title` (default `Untitled`) and `tint` (`default`, `accent`, `success`, `warning`, or `error`; default `default`), and shows its title in a large label colored by its tint. The case places `alpha` (`title: Alpha`, `tint: accent`), `beta` (no props), and `broken` (`title: Broken`, `tint: 42`). Use a fresh home: `YARD_HOME=/tmp/yard-props`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-props LAYOUT_TOUR=props cargo run dev` | `Alpha` in the accent color, `Untitled`, and `Broken` in the default color. One `WARN` inside `props-read{node=broken}`: `prop tint of node broken cannot be read`. |
| Click `rename beta` | `Beta`. `layout-op{op=props, node=beta}` holds `props set {keys=title}`, `props-read`, and `view told {node=beta}`. |
| Click `tint beta warning` | `Beta` in the warning color. |
| Click `clear alpha's tint` | `Alpha` in the default color. |
| Click `props of ghost` | An `INFO`: the node is not in the tree. `user layer: 3`. |
| Quit and start again | `restored {applied=3, skipped=0}`; `Beta` in the warning color, `Alpha` in the default color. `layout.json` holds three `props` operations. |
| `reset layout` | `Alpha` in the accent color and `Untitled` again; `view told` for `alpha` and `beta` inside `layout-reset`. |

### Feature 14: titles

The card `layout_tour.tinted` gives its tab a title from its `title` prop and an icon from its `icon` prop (`none`, `info`, or `warning`). The case places a tabs container with `alpha` (`title: Alpha`, `icon: info`), `beta` (`title: Beta`), and `gamma`, a plain card that gives no title. Use a fresh home: `YARD_HOME=/tmp/yard-titles`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-titles LAYOUT_TOUR=titles cargo run dev` | Tabs `ⓘ Alpha`, `Beta`, and `gamma`; the selected tab's title in the default color, the others muted. `title-register{component=layout_tour.tinted}` with `gives titles`. |
| Click `rename beta` | The tab reads `Bravo`. |
| Drag tab `Alpha` to the end of the bar | It moves, with its title and icon. |
| `reset layout` | `Alpha`, `Beta`, `gamma` again. |

### Feature 15: required-nodes

The component `layout_tour.editor`, a card, is declared required. The case places the controls, then a row with `side` and `main`, a column holding `editor` and `notes`. Use a fresh home: `YARD_HOME=/tmp/yard-required`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-required LAYOUT_TOUR=required cargo run dev` | `side`, then `main` with `editor` and `notes`. `component-require{component=layout_tour.editor}` with `required`. |
| Click `hide editor` | Refused: an `INFO`, `editor cannot be hidden: it is required`. Nothing changes. |
| Click `remove main` | Refused: `main cannot be removed: it holds the required node editor`. |
| Click `hide notes`, then `hide side` | Both are hidden. |
| Click `move editor to root` | `editor` moves to the end of the row; `main` stays, empty, until feature 23 gives empty nodes no room. |

### Feature 16: slot-count

The card `layout_tour.frame` has a slot `content` for one, shown under a small header. The case places the controls, then a row with `frame` (holding `first`) and `loose`, and tries at startup to place `extra` in `frame.content` too. Use a fresh home: `YARD_HOME=/tmp/yard-slot-count`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-slot-count LAYOUT_TOUR=slot-count cargo run dev` | `frame` showing `first`, and `loose`; no `extra`. `slot-declare{component=layout_tour.frame, slot=content}`, and an `INFO` in `node-place{node=extra}`: `frame.content holds one node, and already holds first`. |
| Click `add second to frame` | Refused with the `INFO`. |
| Click `move loose into frame` | Refused with the `INFO`. |
| Click `move first to root` | `first` leaves the frame, which is now empty. |
| Click `move loose into frame` | `loose` is in the frame. |

### Feature 17: placement-order

`layout_tour` places in the row the controls, then cards `a`, `b`, and `c`. `layout_guest`, loaded after it, places its card `guest` before `b`, without knowing how many cards there are. Use a fresh home: `YARD_HOME=/tmp/yard-order`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-order LAYOUT_TOUR=placement-order cargo run dev` | The row: controls, `a`, `guest`, `b`, `c`. `node placed` with `index=2` in `node-place{node=guest}`. |
| Click `c first` | `c` moves to the front of the row, before the controls, which move one place right. |
| Click `a after b` | `a` moves right after `b`. |
| Click `b before ghost` | Refused with an `INFO`: `ghost is not in root.children`. |
| Click `a after a` | Refused with an `INFO`: `a cannot be placed beside itself`. |
| Quit and start again | `restored {applied=2, skipped=0}`; the same order. |
| `reset layout` | Controls, `a`, `guest`, `b`, `c`. |

### Feature 18: contexts

`layout_tour.counter` gives a model `Count`, a number, shown under its header with a `+1` button. `layout_tour.count_card` requires a `Count` and shows the number it receives. The case places the controls, then a row with `counter` holding `left` and `right` (count cards) and `outside`, a plain card. Use a fresh home: `YARD_HOME=/tmp/yard-contexts`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-contexts LAYOUT_TOUR=contexts cargo run dev` | `counter` with `left` and `right`, both showing `0`, and `outside`. `context-declare` for `provide` and `require`; `context created {node=counter, model=Count}`. |
| Click `+1` twice | `left` and `right` both show `2`. |
| Click `move left to root` | Refused: `left requires a Count model, and none is above root`. |
| Click `move outside into counter` | `outside` moves into the counter: it requires nothing. |
| `reset layout` | The default layout; the counter keeps `2`. |

### Feature 19: sizes-as-props

The shares of a split are its `shares` prop, by node ID, so a resize is an operation. The `user-layer` case shows it. Use a fresh home: `YARD_HOME=/tmp/yard-sizes`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-sizes LAYOUT_TOUR=user-layer cargo run dev`, drag the line between `left` and `right` | At the mouse up, one `layout-op{op=props, node=root, extension=yard}` with `props set {keys=shares}`; `user layer: 1`. `layout.json` holds the shares by node ID. |
| Quit and start again | The line where it was left; `restored {applied=1, skipped=0}`. |
| Double-click the line | Equal shares, with `shares reset` and one `layout-op{op=props}`. |
| Drag it again, then `reset layout` | Equal shares; `user layer: 0`. |

### Feature 20: fixed-sizes

A child of a split is fixed when the split's `sizes` prop gives it a size in pixels. The case places the controls above a row with `side`, fixed at 240 px, and `main`, flexible. Use a fresh home: `YARD_HOME=/tmp/yard-fixed`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-fixed LAYOUT_TOUR=fixed-sizes cargo run dev` | `side` shows `240px` wide, `main` the rest. |
| Make the window wider, then narrower | `side` stays `240px`; only `main` changes. |
| Make the window very narrow | `main` stops at its minimum (80 px), then `side` shrinks; widen it again and `side` is `240px` again. |
| Drag the line between `side` and `main` | `side` gets the new width; one `layout-op{op=props, node=row}` with `props set` naming `sizes`. Quit and start: the same width. |
| `reset layout` | `side` is `240px` again. |

### Feature 21: component-sizes

`layout_tour.wide` declares a minimum of 20 rems across, and `layout_tour.panel` a preferred width of 15 rems. The case places the controls above a row with `panel`, `wide`, and `plain`, a card with no hints (its minimum is the default 5 rems). Use a fresh home: `YARD_HOME=/tmp/yard-sizes2`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-sizes2 LAYOUT_TOUR=component-sizes cargo run dev` | `panel` 240 px wide (15 rems at the default font), `wide` and `plain` sharing the rest. `size-hints` for both components. |
| Drag the line between `wide` and `plain` to the left, but not past half of `wide`'s minimum | It stops when `wide` is 320 px (20 rems). Further than half of it, `wide` snaps shut (feature 22). |
| Drag it to the right | `plain` stops at 80 px (5 rems). |
| With `{"theme": {"ui_font_size": 20}}` in `settings.json` and a fresh home | `panel` is 300 px, and `wide` is held at its minimum of 400 px. |
| `reset layout` | `panel` is back at its preferred width. |

### Feature 22: snap-hide

Dragging a split's line past half a child's minimum hides that child; a hidden child of a split leaves a line that drags it out again. The `fixed-sizes` case shows it. Use a fresh home: `YARD_HOME=/tmp/yard-snap`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-snap LAYOUT_TOUR=fixed-sizes cargo run dev`, drag the line between `side` and `main` far to the left | Past half of `side`'s minimum, `snap preview {shut=true}`, and at the mouse up `layout-op{op=hide, node=side, extension=yard}`. `main` takes the whole row. |
| Grab the window's left edge just inside it (the line's hit area is the first 8 px) and drag right | `side` comes back; `layout-op{op=show, node=side}`. |
| Drag it shut, then back before releasing | Nothing is recorded. |
| Drag it shut, quit, start again | `side` is still hidden; drag it out and it is back. |
| `reset layout` | `side` shown at 240 px. |

### Feature 23: empty-nodes

`layout_tour.list` shows its items and says it is empty when it has none. The case places the controls above a row with `left`, a column of two lists, `top` (two items) and `bottom` (none), and `right`, a plain card. Use a fresh home: `YARD_HOME=/tmp/yard-empty`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-empty LAYOUT_TOUR=empty-nodes cargo run dev` | `left` shows only `top`, at the column's full height; `bottom` takes no room. `empty-register{component=layout_tour.list}` with `can be empty`. |
| Click `add to bottom` | `bottom` appears under `top`, and they share the column; `emptiness changed {node=bottom, empty=false}`. |
| Click `clear top`, then `clear bottom` | `top` disappears, then `bottom`, and then the whole `left` column: `right` takes the row (`emptiness changed {node=left, empty=true}`). |
| Click `add to top` | `left` comes back with `top`. |
| `user layer: 0` all along | Nothing was recorded. |

### Feature 24: close-tab

A tab has a close button, shown when the pointer is over the tab, as in Zed; a middle click on a tab closes it too. In the `titles` case, `gamma` is required, so its tab has no button. Use a fresh home: `YARD_HOME=/tmp/yard-close`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-close LAYOUT_TOUR=titles cargo run dev`, rest the pointer on tab `Beta` | A close button appears at its right; resting on it shows `Close Tab`. The selected tab `Alpha` shows none until the pointer is over it, as in Zed. |
| Click `Beta`'s close button | `Beta` goes; `tab close {node=beta}`, `layout-op{op=remove, node=beta, extension=yard}`, `view dropped {node=beta}`. |
| Middle-click tab `Alpha` | `Alpha` goes, and `gamma` is selected. |
| Rest on `gamma` | No close button: it is required. |
| Quit and start again, then `reset layout` | The closed tabs are still closed; after the reset, all three are back. |

### Feature 25: tab-overflow

A tab bar scrolls sideways when its tabs do not fit, and the selected tab is scrolled into view. The case places the controls above a tabs container with twenty cards, `t01` to `t20`. `select t20` reaches `t20` with the shell's tab actions (one `SelectPreviousTab` from `t01`, which wraps). Use a fresh home: `YARD_HOME=/tmp/yard-overflow`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-overflow LAYOUT_TOUR=tab-overflow cargo run dev` | The bar shows the first tabs; the rest are past its end. |
| Scroll over the bar with the trackpad or the wheel | The tabs slide. With `YARD_LIFECYCLE=info,yard_shell=trace`, `tab bar scrolled {tabs, offset}`. |
| Click `select t20` | The bar scrolls so that `t20` is whole at its end; `tab scrolled into view {node=t20}`. |
| Press `alt-cmd-right` (`yard::SelectNextTab`) | `t01` is selected and the bar scrolls back to the start. |
| `user layer: 0` all along | Nothing was recorded. |

### Feature 26: split-on-drop

Dropping a tab on an edge of a container's child splits around it: a new row or column holds the target and a new tabs container with the dropped tab, on the side of the edge. An edge along the parent split's axis still moves the tab into that split, as feature 9. Dropping on the center of a tabs container's content moves the tab into it. The case `user-layer` is used as it is. Use a fresh home: `YARD_HOME=/tmp/yard-splitdrop`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-splitdrop LAYOUT_TOUR=user-layer cargo run dev`, drag tab `b` onto the top edge of `right`'s content | While dragging, the top half of `right`'s content is highlighted. After the drop, a column stands where `right` was: a new tabs container with `b` above `right`; `split-on-drop{node=b, target=right, edge=top}` with four `layout-op` groups; `user layer: 4`. |
| Drag tab `d` onto the center of `left`'s content | The whole of `left`'s content is highlighted; `d` moves into `left`, last, and is selected. |
| Quit and start again | The same layout: `restored {applied=5, skipped=0}`. |
| `reset layout` | `left` with `a`, `b` and `right` with `c`, `d` again. |

### Feature 27: toggle

Buttons in the status bar show and hide nodes, as Zed's dock buttons show and hide its panels. The body is a row: a column `side` with cards `left` (shown) and `extra` (hidden at start), then `main`, a column of controls with `reset layout`, then card `right`. The status bar holds, on its left, a toggle for `left` (icon `info`) and one for `extra` (icon `plus`), both with `alone`, and on its right a toggle for `right` (icon `warning`). Use a fresh home: `YARD_HOME=/tmp/yard-toggle`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-toggle LAYOUT_TOUR=toggle cargo run dev` | The status bar shows three buttons; those of `left` and `right` are toggled, that of `extra` is not. |
| Click the first toggle | `left` hides; the button is no longer toggled; `toggle{node=left, shown=false}` with one `layout-op{op=hide}`. |
| Click it again | `left` shows. |
| Click `extra`'s toggle | `left` hides and then `extra` shows, in `side`; two `layout-op` groups inside `toggle{node=extra, shown=true}`. |
| Quit and start again, then `reset layout` | The same state; after the reset, `left` and `right` shown and `extra` hidden. |

### Feature 28: item-slots

A slot shows one copy of its template for each item of a model, and items come and go without layout operations. The case's model `Names` starts with `ada`, `bob`, `cy`; the row `names_row` is an item slot over it, with one template, the card `name`, whose copies `name@<key>` show their key. Use a fresh home: `YARD_HOME=/tmp/yard-items`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-items LAYOUT_TOUR=item-slots cargo run dev` | Three cards in a row: `ada`, `bob`, `cy`. `item-slot-declare{node=names_row, slot=children, model=Names}`. |
| Click `count` on `bob`'s card twice, then `add dee` | A fourth card `dee`; `items-changed{added=dee}` with one `item-copy{key=dee}`; `bob` still shows `count: 2`. |
| Click `remove ada` | `ada`'s card goes; `view dropped {node=name@ada}`. |
| Click `reverse` | The cards are in the reverse order; `bob` still shows `count: 2`. |
| Click `zoom` on a card | Nothing zooms: `name@cy is a copy of an item slot`, an `INFO`. |
| `user layer: 0` all along | Nothing was recorded. |

### Feature 29: item-tabs

A tabs container over an item slot is a tab bar of its model's items, as Zed's pane is a tab bar of its open items: each tab has a close button and the bar ends with a `+`. The buttons only ask the model. The case's model `Names` starts with `ada`, `bob`, `cy`; it closes an item by removing its key, except `ada`, which it keeps, and makes a new item `new-<n>`. Each card is titled by its key. Use a fresh home: `YARD_HOME=/tmp/yard-itemtabs`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-itemtabs LAYOUT_TOUR=item-tabs cargo run dev` | Three tabs, `ada`, `bob`, `cy`, and a `+` at the bar's end. |
| Rest on `bob`'s tab, click its close button | `bob`'s tab goes; `item-close{key=bob}` holding `items-changed{removed=bob}` and `view dropped {node=name@bob}`. |
| Click `ada`'s close button | `ada` stays; `items unchanged`. |
| Click `+` twice | Tabs `new-1` and `new-2` appear, each selected when it appears. |
| `user layer: 0` all along | Nothing was recorded. |

### Feature 30: named-layouts

An extension registers named layouts, each a list of operations over the default; the window shows one at a time and switches between them, keeping the views of the nodes both show. Each named layout has its own user layer and file. The case is a row of cards `a`, `b`, `c` beside controls: `default`, `swap`, `focus`, `hide c`, `reset layout`, and the shown layout's name. `layout_tour.swap` moves `c` before `a`; `layout_tour.focus` hides `a` and `c` and adds a card `d`. Use a fresh home: `YARD_HOME=/tmp/yard-named`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-named LAYOUT_TOUR=named-layouts cargo run dev` | `a`, `b`, `c`; shown: `yard.default`. Two `named-layout-register` groups. |
| Click `count` on `b` twice, then `swap` | `c`, `a`, `b`; `b` still shows `count: 2`; `layout-switch{from=yard.default, to=layout_tour.swap}` with `switched {kept=…, dropped=0, created=0}`. |
| Click `focus` | `b` and `d` only; `b` still shows `count: 2`; `d` is created, `a` and `c` hidden. |
| Click `swap`, then `hide c`, then `default` | In `swap`, `c` hides; back in `default`, `c` is shown: the hide is in `layouts/layout_tour.swap.json` only. |
| Click `swap` again, then `reset layout` | `c` is back, before `a`: only `swap`'s user layer was dropped. |

### Feature 31: inspector

With `LAYOUT_INSPECTOR` set, every case's root is placed in a row `tour_frame`, and the extension `layout_inspector` shows the layout tree in that row, on the right. Its walkthrough is in `examples/layout_inspector/README.md`. Without it, every case is as before.

### Window feature 8: window-tree

The layout belongs to its window. With one window, every case runs as before; the log names the window: `shell-create{root, window}`, each operation, reset, restore, and drop inside `window{id=…}`, and `window-close{window}` with `view dropped` when the window closes. `YARD_HOME=/tmp/yard-wtree LAYOUT_TOUR=user-layer cargo run dev`, drag tab `b` onto the top edge of `right`'s content, then `reset layout`: the same as in feature 26, inside `window{id=…}`.

### Window feature 9: new-window

`yard::NewWindow` opens another window with the default layout, rearranged on its own. Use a fresh home: `YARD_HOME=/tmp/yard-newwin`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-newwin LAYOUT_TOUR=user-layer cargo run dev`, press `cmd-shift-n` | A second window with the default layout, cascaded from the first and as large; `window-new`. |
| Drag tab `b` onto the top edge of `right` in the second window | Only the second window splits (`user layer: 4`); the first shows `user layer: 0`. |
| `File > New Window` | A third window. |
| Close the windows one by one | `window-close{window}` for each; the application quits with the last. |

### Window feature 10: restore-windows

The windows open at quit come back at the next start, each with its place, its size, its named layout, and its own user layers, from `session.json`. Use a fresh home: `YARD_HOME=/tmp/yard-session`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-session LAYOUT_TOUR=user-layer cargo run dev`, click `hide d`, press `cmd-shift-n`, move the new window, drag tab `b` onto the top edge of `right` in it, quit | `session-save{windows=2}`; `session.json` lists both windows. |
| Start again | Both windows are back where they were: the first with `d` hidden, the second split; `restored {windows=2}`. |
| Close the second window, quit, start | One window. |

### State feature 1: node-state

The card keeps its count across a restart: it registers its count as its node's state, saved to `state.json` when the application quits and given back when the card's view is first created. Each window keeps its own cards' counts, and the `user layer: N` label shows its own window's count. Use a fresh home: `YARD_HOME=/tmp/yard-state`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-state LAYOUT_TOUR=user-layer cargo run dev`, click `count` on `a` three times, quit with `cmd-q` | `state-save{windows=1, nodes=4}`; `state.json` holds `a`'s `count: 3`. |
| Start again | `a` shows `count: 3`; `state given {node=a}`. |
| Press `cmd-shift-n`, count `c` in the new window twice, quit, start | Both windows are back, each with its own counts and its own `user layer` count. |

### State feature 2: save-on-change

The card says when its count changed, and the states are saved a short time later: 300 ms after the last change, and never more than 2 s after the first change of a burst. A crash loses at most the last 2 s. Opening the window also saves once, since a window opening counts as a change. Use a fresh home: `YARD_HOME=/tmp/yard-soc`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-soc LAYOUT_TOUR=user-layer cargo run dev`, click `count` on `a` ten times quickly | One `state-save{windows=1, reason=change}` after the clicks stop. Clicks spread over more than 2 s save once more in between. |
| End the process with `kill -9` | `state.json` holds `a`'s `count: 10`. |
| Start again | `a` shows `count: 10`; `state given {node=a}`. |

### State feature 3: view-state

The shell's own containers keep their state as the cards do: each tabs container its selected tab, and the window the node that fills it. Use a fresh home: `YARD_HOME=/tmp/yard-view`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-view LAYOUT_TOUR=user-layer cargo run dev`, select the tab `d`, click `zoom` on `d`, quit | `state.json` holds `right`: `{"selected": "d"}` and `window`: `{"zoomed": "d"}`. |
| Start again | `d` fills the window; `zoom given {node=d}` and `tab given {tabs=right, tab=d}`. Click `zoom` again: `d` is the selected tab of `right`. |
| Zoom out, select `c`, end the process with `kill -9`, start | `c` is selected, nothing is zoomed. |

### State feature 4: extension-store

The tour keeps a value of its own, outside any node: in the case `user-layer`, the controls show `starts: N`, and each start adds one to the tour's stored `starts`. Use a fresh home: `YARD_HOME=/tmp/yard-store`.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-store LAYOUT_TOUR=user-layer cargo run dev` | `starts: 1`; `store-read{extension=layout_tour}` with `no saved values`, then `store-save{extension=layout_tour, keys=1, reason=change}`. |
| Quit, start again | `starts: 2`; `read {keys=1}`; `store/layout_tour.json` holds `"starts": 2`. |
| End the process with `kill -9` after a second, start again | `starts: 3`. |

### Packaging feature 3: open-urls

In the case `user-layer`, the tour handles the URLs that name it and its controls show `url: <last URL>`. macOS sends URLs only to a bundle, so this case runs from a dev bundle with its own name and scheme (`yard-check`), never the real ones. Make a config `/tmp/yard-check.toml` with `name = "yard-check"`, `version = "0.1.0"`, `identifier = "vn.risotech.yard-check"`, `icon = "icon.png"` (copy `app/yard/icon.png` to `/tmp/icon.png`: the icon's path is relative to the config), and `url_scheme = "yard-check"`.

| Command and action | What to look for |
|---|---|
| `script/bundle-macos --dev --env YARD_CHECK_HOME=/tmp/yard-urls --env LAYOUT_TOUR=user-layer --config /tmp/yard-check.toml`, then `open dist/yard-check.app` | The tour's window, with its home in `/tmp/yard-urls`. |
| `open "yard-check://layout_tour/hello?token=secret"` | The controls show `url: yard-check://layout_tour/hello?token=secret`; no second copy starts; the log has `url-open{scheme=yard-check, host=layout_tour}` and `handled`, and no `secret`. |
| Quit, then `open "yard-check://layout_tour/first"` | The application starts and shows `url: yard-check://layout_tour/first`, with `kept until extensions load` and `handed over {count=1}`. |
| `open "yard-check://nobody/x"` | `refused` with the reason that no extension is named `nobody`. |

## Keymap feature 1: focused-node

The card shows `focused` when its node has the keyboard focus, and `contains focus` when a node inside it has it. Use a fresh home: `YARD_HOME=/tmp/yard-layout`.

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

Remove it from the list of extensions in `app/yard/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/yard/Cargo.toml`, and the app runs as it should without it.

## Keymap feature 6: user-keymap

With a `keymap.json` in the fresh home's `config/`, the user's keys act on the `zoom` case's cards. Use `YARD_HOME=/tmp/yard-keys`.

| Command and action | What to look for |
|---|---|
| Write `[{"context": "Shell", "bindings": {"cmd-shift-z": "yard::ToggleZoom", "shift-escape": null}}]` to `config/keymap.json`; `LAYOUT_TOUR=zoom cargo run dev`; click card `b`; press `cmd-shift-z` | `b` zooms. `keymap-load` with two `bound` lines and `user keys bound {count=2, skipped=0}`. |
| Press `shift-escape` | Nothing: the default key is removed. |
| Add an entry with an unknown action and one with keys `cmd-+-x` | Each a `WARN` naming it; the good entries still work. |

## Keymap feature 7: keymap-reload

`keymap.json` is read again within a second of a change, while yard runs.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=zoom cargo run dev` with no `keymap.json`; click card `b`; write `[{"context": "Shell", "bindings": {"cmd-shift-z": "yard::ToggleZoom"}}]` to `config/keymap.json`; press `cmd-shift-z` | `b` zooms, without restarting. `keymap-reload` with `defaults bound` and `keymap-load` inside it. |
| Remove that binding from the file; press `cmd-shift-z` | Nothing. `shift-escape` still zooms. |
| Break the file (delete a bracket) | The `WARN`; the default keys still work. |
