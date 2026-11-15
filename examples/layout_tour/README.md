# layout_tour

A test extension for the layout engine. When `app/riso/src/dev/review.rs` lists it as `("layout_tour", layout_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the layout engine again, or when the next feature that it shows is under review.

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

### Feature 13: props

The card `layout_tour.tinted` takes props `title` (default `Untitled`) and `tint` (`default`, `accent`, `success`, `warning`, or `error`; default `default`), and shows its title in a large label colored by its tint. The case places `alpha` (`title: Alpha`, `tint: accent`), `beta` (no props), and `broken` (`title: Broken`, `tint: 42`). Use a fresh home: `RISO_HOME=/tmp/riso-props`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-props LAYOUT_TOUR=props cargo run dev` | `Alpha` in the accent color, `Untitled`, and `Broken` in the default color. One `WARN` inside `props-read{node=broken}`: `prop tint of node broken cannot be read`. |
| Click `rename beta` | `Beta`. `layout-op{op=props, node=beta}` holds `props set {keys=title}`, `props-read`, and `view told {node=beta}`. |
| Click `tint beta warning` | `Beta` in the warning color. |
| Click `clear alpha's tint` | `Alpha` in the default color. |
| Click `props of ghost` | An `INFO`: the node is not in the tree. `user layer: 3`. |
| Quit and start again | `restored {applied=3, skipped=0}`; `Beta` in the warning color, `Alpha` in the default color. `layout.json` holds three `props` operations. |
| `reset layout` | `Alpha` in the accent color and `Untitled` again; `view told` for `alpha` and `beta` inside `layout-reset`. |

### Feature 14: titles

The card `layout_tour.tinted` gives its tab a title from its `title` prop and an icon from its `icon` prop (`none`, `info`, or `warning`). The case places a tabs container with `alpha` (`title: Alpha`, `icon: info`), `beta` (`title: Beta`), and `gamma`, a plain card that gives no title. Use a fresh home: `RISO_HOME=/tmp/riso-titles`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-titles LAYOUT_TOUR=titles cargo run dev` | Tabs `ⓘ Alpha`, `Beta`, and `gamma`; the selected tab's title in the default color, the others muted. `title-register{component=layout_tour.tinted}` with `gives titles`. |
| Click `rename beta` | The tab reads `Bravo`. |
| Drag tab `Alpha` to the end of the bar | It moves, with its title and icon. |
| `reset layout` | `Alpha`, `Beta`, `gamma` again. |

### Feature 15: required-nodes

The component `layout_tour.editor`, a card, is declared required. The case places the controls, then a row with `side` and `main`, a column holding `editor` and `notes`. Use a fresh home: `RISO_HOME=/tmp/riso-required`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-required LAYOUT_TOUR=required cargo run dev` | `side`, then `main` with `editor` and `notes`. `component-require{component=layout_tour.editor}` with `required`. |
| Click `hide editor` | Refused: an `INFO`, `editor cannot be hidden: it is required`. Nothing changes. |
| Click `remove main` | Refused: `main cannot be removed: it holds the required node editor`. |
| Click `hide notes`, then `hide side` | Both are hidden. |
| Click `move editor to root` | `editor` moves to the end of the row; `main` stays, empty, until feature 23 gives empty nodes no room. |

### Feature 16: slot-count

The card `layout_tour.frame` has a slot `content` for one, shown under a small header. The case places the controls, then a row with `frame` (holding `first`) and `loose`, and tries at startup to place `extra` in `frame.content` too. Use a fresh home: `RISO_HOME=/tmp/riso-slot-count`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-slot-count LAYOUT_TOUR=slot-count cargo run dev` | `frame` showing `first`, and `loose`; no `extra`. `slot-declare{component=layout_tour.frame, slot=content}`, and an `INFO` in `node-place{node=extra}`: `frame.content holds one node, and already holds first`. |
| Click `add second to frame` | Refused with the `INFO`. |
| Click `move loose into frame` | Refused with the `INFO`. |
| Click `move first to root` | `first` leaves the frame, which is now empty. |
| Click `move loose into frame` | `loose` is in the frame. |

### Feature 17: placement-order

`layout_tour` places in the row the controls, then cards `a`, `b`, and `c`. `layout_guest`, loaded after it, places its card `guest` before `b`, without knowing how many cards there are. Use a fresh home: `RISO_HOME=/tmp/riso-order`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-order LAYOUT_TOUR=placement-order cargo run dev` | The row: controls, `a`, `guest`, `b`, `c`. `node placed` with `index=2` in `node-place{node=guest}`. |
| Click `c first` | `c` moves to the front of the row, before the controls, which move one place right. |
| Click `a after b` | `a` moves right after `b`. |
| Click `b before ghost` | Refused with an `INFO`: `ghost is not in root.children`. |
| Click `a after a` | Refused with an `INFO`: `a cannot be placed beside itself`. |
| Quit and start again | `restored {applied=2, skipped=0}`; the same order. |
| `reset layout` | Controls, `a`, `guest`, `b`, `c`. |

### Feature 18: contexts

`layout_tour.counter` gives a model `Count`, a number, shown under its header with a `+1` button. `layout_tour.count_card` requires a `Count` and shows the number it receives. The case places the controls, then a row with `counter` holding `left` and `right` (count cards) and `outside`, a plain card. Use a fresh home: `RISO_HOME=/tmp/riso-contexts`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-contexts LAYOUT_TOUR=contexts cargo run dev` | `counter` with `left` and `right`, both showing `0`, and `outside`. `context-declare` for `provide` and `require`; `context created {node=counter, model=Count}`. |
| Click `+1` twice | `left` and `right` both show `2`. |
| Click `move left to root` | Refused: `left requires a Count model, and none is above root`. |
| Click `move outside into counter` | `outside` moves into the counter: it requires nothing. |
| `reset layout` | The default layout; the counter keeps `2`. |

### Feature 19: sizes-as-props

The shares of a split are its `shares` prop, by node ID, so a resize is an operation. The `user-layer` case shows it. Use a fresh home: `RISO_HOME=/tmp/riso-sizes`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-sizes LAYOUT_TOUR=user-layer cargo run dev`, drag the line between `left` and `right` | At the mouse up, one `layout-op{op=props, node=root, extension=riso}` with `props set {keys=shares}`; `user layer: 1`. `layout.json` holds the shares by node ID. |
| Quit and start again | The line where it was left; `restored {applied=1, skipped=0}`. |
| Double-click the line | Equal shares, with `shares reset` and one `layout-op{op=props}`. |
| Drag it again, then `reset layout` | Equal shares; `user layer: 0`. |

### Feature 20: fixed-sizes

A child of a split is fixed when the split's `sizes` prop gives it a size in pixels. The case places the controls above a row with `side`, fixed at 240 px, and `main`, flexible. Use a fresh home: `RISO_HOME=/tmp/riso-fixed`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-fixed LAYOUT_TOUR=fixed-sizes cargo run dev` | `side` shows `240px` wide, `main` the rest. |
| Make the window wider, then narrower | `side` stays `240px`; only `main` changes. |
| Make the window very narrow | `main` stops at its minimum (80 px), then `side` shrinks; widen it again and `side` is `240px` again. |
| Drag the line between `side` and `main` | `side` gets the new width; one `layout-op{op=props, node=row}` with `props set` naming `sizes`. Quit and start: the same width. |
| `reset layout` | `side` is `240px` again. |

### Feature 21: component-sizes

`layout_tour.wide` declares a minimum of 20 rems across, and `layout_tour.panel` a preferred width of 15 rems. The case places the controls above a row with `panel`, `wide`, and `plain`, a card with no hints (its minimum is the default 5 rems). Use a fresh home: `RISO_HOME=/tmp/riso-sizes2`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-sizes2 LAYOUT_TOUR=component-sizes cargo run dev` | `panel` 240 px wide (15 rems at the default font), `wide` and `plain` sharing the rest. `size-hints` for both components. |
| Drag the line between `wide` and `plain` to the left, but not past half of `wide`'s minimum | It stops when `wide` is 320 px (20 rems). Further than half of it, `wide` snaps shut (feature 22). |
| Drag it to the right | `plain` stops at 80 px (5 rems). |
| With `{"theme": {"ui_font_size": 20}}` in `settings.json` and a fresh home | `panel` is 300 px, and `wide` is held at its minimum of 400 px. |
| `reset layout` | `panel` is back at its preferred width. |

### Feature 22: snap-hide

Dragging a split's line past half a child's minimum hides that child; a hidden child of a split leaves a line that drags it out again. The `fixed-sizes` case shows it. Use a fresh home: `RISO_HOME=/tmp/riso-snap`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-snap LAYOUT_TOUR=fixed-sizes cargo run dev`, drag the line between `side` and `main` far to the left | Past half of `side`'s minimum, `snap preview {shut=true}`, and at the mouse up `layout-op{op=hide, node=side, extension=riso}`. `main` takes the whole row. |
| Grab the window's left edge just inside it (the line's hit area is the first 8 px) and drag right | `side` comes back; `layout-op{op=show, node=side}`. |
| Drag it shut, then back before releasing | Nothing is recorded. |
| Drag it shut, quit, start again | `side` is still hidden; drag it out and it is back. |
| `reset layout` | `side` shown at 240 px. |

### Feature 23: empty-nodes

`layout_tour.list` shows its items and says it is empty when it has none. The case places the controls above a row with `left`, a column of two lists, `top` (two items) and `bottom` (none), and `right`, a plain card. Use a fresh home: `RISO_HOME=/tmp/riso-empty`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-empty LAYOUT_TOUR=empty-nodes cargo run dev` | `left` shows only `top`, at the column's full height; `bottom` takes no room. `empty-register{component=layout_tour.list}` with `can be empty`. |
| Click `add to bottom` | `bottom` appears under `top`, and they share the column; `emptiness changed {node=bottom, empty=false}`. |
| Click `clear top`, then `clear bottom` | `top` disappears, then `bottom`, and then the whole `left` column: `right` takes the row (`emptiness changed {node=left, empty=true}`). |
| Click `add to top` | `left` comes back with `top`. |
| `user layer: 0` all along | Nothing was recorded. |

### Feature 24: close-tab

A tab has a close button, shown when the pointer is over the tab, as in Zed; a middle click on a tab closes it too. In the `titles` case, `gamma` is required, so its tab has no button. Use a fresh home: `RISO_HOME=/tmp/riso-close`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-close LAYOUT_TOUR=titles cargo run dev`, rest the pointer on tab `Beta` | A close button appears at its right; resting on it shows `Close Tab`. The selected tab `Alpha` shows none until the pointer is over it, as in Zed. |
| Click `Beta`'s close button | `Beta` goes; `tab close {node=beta}`, `layout-op{op=remove, node=beta, extension=riso}`, `view dropped {node=beta}`. |
| Middle-click tab `Alpha` | `Alpha` goes, and `gamma` is selected. |
| Rest on `gamma` | No close button: it is required. |
| Quit and start again, then `reset layout` | The closed tabs are still closed; after the reset, all three are back. |

### Feature 25: tab-overflow

A tab bar scrolls sideways when its tabs do not fit, and the selected tab is scrolled into view. The case places the controls above a tabs container with twenty cards, `t01` to `t20`. `select t20` reaches `t20` with the shell's tab actions (one `SelectPreviousTab` from `t01`, which wraps). Use a fresh home: `RISO_HOME=/tmp/riso-overflow`.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-overflow LAYOUT_TOUR=tab-overflow cargo run dev` | The bar shows the first tabs; the rest are past its end. |
| Scroll over the bar with the trackpad or the wheel | The tabs slide. With `RISO_LIFECYCLE=info,riso_shell=trace`, `tab bar scrolled {tabs, offset}`. |
| Click `select t20` | The bar scrolls so that `t20` is whole at its end; `tab scrolled into view {node=t20}`. |
| Press `alt-cmd-right` (`riso::SelectNextTab`) | `t01` is selected and the bar scrolls back to the start. |
| `user layer: 0` all along | Nothing was recorded. |

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

## Keymap feature 6: user-keymap

With a `keymap.json` in the fresh home's `config/`, the user's keys act on the `zoom` case's cards. Use `RISO_HOME=/tmp/riso-keys`.

| Command and action | What to look for |
|---|---|
| Write `[{"context": "Shell", "bindings": {"cmd-shift-z": "riso::ToggleZoom", "shift-escape": null}}]` to `config/keymap.json`; `LAYOUT_TOUR=zoom cargo run dev`; click card `b`; press `cmd-shift-z` | `b` zooms. `keymap-load` with two `bound` lines and `user keys bound {count=2, skipped=0}`. |
| Press `shift-escape` | Nothing: the default key is removed. |
| Add an entry with an unknown action and one with keys `cmd-+-x` | Each a `WARN` naming it; the good entries still work. |

## Keymap feature 7: keymap-reload

`keymap.json` is read again within a second of a change, while riso runs.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=zoom cargo run dev` with no `keymap.json`; click card `b`; write `[{"context": "Shell", "bindings": {"cmd-shift-z": "riso::ToggleZoom"}}]` to `config/keymap.json`; press `cmd-shift-z` | `b` zooms, without restarting. `keymap-reload` with `defaults bound` and `keymap-load` inside it. |
| Remove that binding from the file; press `cmd-shift-z` | Nothing. `shift-escape` still zooms. |
| Break the file (delete a bracket) | The `WARN`; the default keys still work. |
