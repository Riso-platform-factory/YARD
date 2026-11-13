# Window engine feature 1: title-bar

## What it adds

The window draws its own title bar, as Zed's does, and extensions put items in it. For this, the tree of a window starts with a root of the shell's own, the window's frame. Under it are two parts:

- the title bar, with the slots `left`, `center`, and `right`;
- the body, where an extension's root goes, as today.

The status bar is the next feature, `status-bar`.

## The frame

- **The shell places the frame before any extension places a node.** The shell's components, all in the namespace `riso`, are registered as the row, column, and tabs are:

  | Node | Component | Slots | Where |
  |---|---|---|---|
  | `window` | `riso.window` | `title_bar`, `body` | the tree's root |
  | `title_bar` | `riso.title_bar` | `left`, `center`, `right` | slot `title_bar` of `window` |
  | `body` | `riso.body` | `children` | slot `body` of `window` |

- **The frame belongs to the shell.** Each of these is refused at `info` with the reason `the window's frame belongs to the shell`, whether it comes from an extension at startup, at run time, by drag and drop, or from the saved layout being restored:
  - removing, moving, hiding, showing, or zooming `window`, `title_bar`, or `body`;
  - placing, adding, or moving any node into the slots `title_bar` and `body` of `window`, or into the slot `children` of `body`. Only the shell fills them, and only `place_root` puts a node in `body`;
  - placing or adding a node of the components `riso.window`, `riso.title_bar`, or `riso.body`.

  The frame's IDs are taken: a node placed with one of them is refused as any duplicate ID is. The frame's nodes take no focus, and `focus_handle` gives none for them.
- **An extension's root goes in the body.** `place_root` places the node in the slot `children` of `body`, with the same group, `root-place`, and the same refusal when a root is already placed. The shell remembers which node `place_root` placed: that node is the root, and stays the only node in the body. It keeps every protection of a root: it cannot be removed, moved, or hidden, and zooming it is refused with `the root already fills the body`.
- **Items in the bar are ordinary nodes.** An extension places a node of its own component in a slot of `title_bar` with `place` or `add`, as in any other node's slot, and can hide, show, move, and remove it under the usual rules, including moving it between the bar and a container in the body.
- **Zoom fills the body.** A zoomed node is drawn in the body, and the title bar stays, as in Zed. Only a node in the body can be zoomed: zooming a bar item is refused at `info` with `only a node in the body can be zoomed`. Focus by direction, `node_bounds`, and the focused node treat the frame as containers: a bar item is a leaf, and can take the focus.
- **The user's layer and its saving** do not change: the frame is in the default tree, never in the user's changes. A layout file saved before this feature, whose changes name only the extensions' nodes, restores as before.

## How the title bar looks and works

As Zed's `PlatformTitleBar` on macOS, from `crates/platform_title_bar/src/platform_title_bar.rs`, ported through the earlier project's `crew_ui` title bar:

- **The window is opened with a transparent system title bar,** with the traffic lights at (9, 9), as Zed opens its windows. The bar is drawn by riso.
- **Its height is Zed's:** 1.75 rems, at least 34px. It follows the UI font size, not the density.
- **Its background is the theme's title bar background.** On macOS, the left of the bar keeps room for the traffic lights, unless the window is full screen or in simple full screen, as Zed checks both.
- **The slots are laid out as Zed lays out its title bar's children:** `left` at the start, `center` in the middle, `right` at the end, each a row of its items, vertically centred.
- **With nothing shown in `left`, the bar shows the window's title** there, in a small label, as Zed shows its project's name. The startup gives the product's name as the title, so a plain start shows `riso`.
- **Dragging the bar moves the window,** and double-clicking it does what macOS is set to do, zoom or minimize, through the OS. A click on an item in the bar reaches the item and does not move the window.
- The body fills the rest of the window, below the bar.

## What code can do

```rust
// riso_ui
pub fn titlebar_options(title: impl Into<SharedString>) -> TitlebarOptions;
pub struct TitleBar;   // TitleBar::new(id).left(..).center(..).right(..) — IntoElement

// riso_shell
pub fn set_window_title(cx: &mut App, title: impl Into<SharedString>);
```

The node IDs `window`, `title_bar`, and `body` and the slot names are the API by which extensions place items.

## Logging

| Situation | Log |
|---|---|
| The shell places its frame | A group `window-frame`, with the `root-place` and `node-place` groups of its three nodes inside, as any placement logs |
| The title is set | `debug`: `window title {title}` |
| An extension changes the frame | `info`: refused, `the window's frame belongs to the shell`, inside the operation's group; from a saved layout, inside `layout-restore`, as its other refusals are |
| Zooming a bar item | `info`: refused, `only a node in the body can be zoomed` |
| Zooming the extension's root | `info`: refused, `the root already fills the body` |
| The title bar draws | Exactly one `trace` line in all, from the shell: the number of shown items in each slot, and whether the title is shown. riso_ui's `TitleBar` logs nothing of its own |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/title_bar` (new, dev) | `TitleBar` and `titlebar_options`, adapted from Zed through crew, with Zed's notice |
| `riso_shell/shell` | Dev version beside production. The frame's components and nodes, the rules that keep the frame the shell's, `place_root` into the body, zoom into the body, `set_window_title` |
| `riso/startup` | Dev copy. It opens the window with `titlebar_options` and sets the window title to the product's name |

## Tests

- GPUI tests, in a window of a known size:
  - the title bar is at the top, the window's full width, 34px high at a 16px rem; the body is below it and fills the rest;
  - an extension's root placed with `place_root` fills the body;
  - with nothing in `left`, the title is drawn in the bar, with the text given by `set_window_title`, checked on screen and not only in the log; a node placed in `left` replaces it; hiding that node shows the title again, and showing it hides the title, each checked after its own draw;
  - items with a width of their own in `left`, `center`, and `right` are drawn in that order from left to right, inside the bar, without overlapping;
  - a click on a button in the bar calls the button's `on_click` once;
  - a zoomed node fills the body, and the title bar is still drawn; zooming a bar item is refused, and the item stays in the bar;
  - removing, moving, hiding, showing, and zooming each of `window`, `title_bar`, and `body` is refused, each checked after its own draw;
  - placing at startup, adding at run time, and moving a node into each of the frame's three slots is refused, and so is placing a node of each frame component;
  - moving a node of the body into `body.children`, ahead of the root, is refused; after that attempt, the root is still protected;
  - a saved layout whose changes move a node into a frame slot, or change a frame node, restores without those changes;
  - a saved layout of production's shell, with changes to the extension's nodes, restores them;
  - `focus_handle` gives none for the frame's nodes;
  - focus by direction from the root's card upwards reaches a bar item, and never the frame nodes.
- A logging test alone in its file: every row of the table. The title bar's draw line is counted over every crate's lines, not only the shell's: exactly one per draw.
- **Existing tests:** only these change, and each change is listed when it is made:
  - a test that measured an extension's root, or a zoomed node, against the whole window now measures it against the body;
  - a test that lists every line of a run, or every component created, in order, now also has the frame's: the `window-frame` group, the root placed in `body`, and the `component-create` of `window`, `title_bar`, and `body`;
  - a test that reads the refusal of zooming the root now reads `the root already fills the body`.

## Test extension: `examples/window_tour`

A new test extension, `window_tour`. With `WINDOW_TOUR=title-bar`, it places a card as the root, a label `window_tour` in the bar's `left`, and a button `ping` in the bar's `right` that logs `ping`. Without `WINDOW_TOUR`, it does nothing. Use a fresh home: `RISO_HOME=/tmp/riso-window`.

| Command and action | What to look for |
|---|---|
| `cargo run dev` | The bar, drawn by riso, with the traffic lights and `riso` on the left. `window-frame` in the log. |
| `WINDOW_TOUR=title-bar cargo run dev` | `window_tour` on the left instead of `riso`, and `ping` on the right. |
| Click `ping` | `ping` in the log, and the window does not move. |
| Drag the bar | The window moves. Double-click it: the window zooms, as macOS is set. |
| `LAYOUT_TOUR=zoom cargo run dev`, zoom `c` | `c` fills the body, under the bar. |
| `{"theme": {"mode": "light"}}`, and `{"theme": {"ui_font_size": 20}}` | The bar follows the theme, and grows with the font size. |

## Under review

- `riso_ui/title_bar`, new
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- The status bar: `status-bar`.
- Window buttons drawn by riso on Linux and Windows, and Zed's tabs of macOS windows: later.
- Zed's own handling of the drag, with `start_window_move`: the OS's drag is used.
