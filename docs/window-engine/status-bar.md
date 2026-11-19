# Window engine feature 2: status-bar

## What it adds

The window's frame gets its third part, the status bar, at the bottom of the window, as Zed's. Extensions put items in it, on its left and on its right. While nothing is shown in it, it takes no room.

## The frame

- **The shell places a fourth frame node,** in the same `window-frame` group, after `body`:

  | Node | Component | Slots | Where |
  |---|---|---|---|
  | `status_bar` | `yard.status_bar` | `left`, `right` | slot `status_bar` of `window`, a new slot after `body` |

- **It belongs to the shell,** under every rule of the frame: it cannot be removed, moved, hidden, shown, or zoomed; nothing is placed, added, or moved into the slot `status_bar` of `window`; no extension places a node of `yard.status_bar`; its ID is taken; it takes no focus. Each refusal is the frame's, `the window's frame belongs to the shell`, on every path, the saved layout's restore included.
- **Items in the status bar are ordinary nodes,** as the title bar's are: an extension places a node of its own component in `left` or `right` with `place` or `add`, and can hide, show, move, and remove it under the usual rules, including moving it to the title bar or into the body. Zooming a status bar item is refused, as a bar item's is: `only a node in the body can be zoomed`.
- **Zoom fills the body,** and the status bar stays, as in Zed, where the zoomed item covers the center and the status bar stays below it.
- **Focus by direction** treats `status_bar` as a container: from the root's card downwards, it reaches a status bar item; it never reaches the frame's nodes.

## How the status bar looks and works

As Zed's `StatusBar`, from `crates/workspace/src/status_bar.rs`, ported through the earlier project's `crew_ui` status bar:

- **A row the window's full width,** below the body: the background is the theme's status bar background; the padding is Zed's `Base04` and the gap between its two sides Zed's `Base08`, both following the density.
- **The left side** is a row of the `left` items with Zed's `gap_1`, which may shrink and clips what does not fit. **The right side** is a row of the `right` items with `gap_1`, which never shrinks, at the right end.
- **Items are drawn in their slot's order, from left to right,** on both sides. Zed draws its right items in reverse because it adds each new one at the end of a list; YARD's slots have an order of their own, which is the one drawn.
- **While no item of either slot is shown, the status bar is not drawn and takes no room:** the body reaches the bottom of the window. When an item is shown, hidden, placed, or removed, the bar appears or goes at the next draw, and the body shrinks or grows.
- Its items are vertically centred; the bar's height is that of its tallest item plus the padding.
- Zed's user setting that hides the status bar, its right-click menu to hide an item, and its keyboard navigation between items are not part of this feature.

## What code can do

```rust
// yard_ui
pub struct StatusBar;   // StatusBar::new(id).left(..).right(..) — IntoElement
```

The node ID `status_bar` and the slots `left` and `right` are the API by which extensions place items.

## Logging

| Situation | Log |
|---|---|
| The shell places its frame | `window-frame` also holds the `node-place` group of `status_bar` |
| An extension changes the status bar node or its slot | `info`: refused, `the window's frame belongs to the shell`, as for the other frame nodes |
| Zooming a status bar item | `info`: refused, `only a node in the body can be zoomed` |
| The status bar draws | Exactly one `trace` line in all, from the shell: the number of shown items in each slot. While it is not drawn, it logs nothing. yard_ui's `StatusBar` logs nothing of its own |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/status_bar` (new, dev) | `StatusBar`, adapted from Zed through crew, with Zed's notice |
| `yard_shell/shell` | Dev version beside production. The fourth frame node and its slot, its rules, and the window drawing it below the body |

The startup does not change.

## Tests

- GPUI tests, in a window of a known size:
  - with no status bar item, nothing is drawn below the body, and the body reaches the bottom of the window;
  - with an item in `left`, the bar is at the bottom, the window's full width, below the body, and the body ends where the bar starts;
  - items with a width of their own in `left` are drawn from the bar's left edge in their order, and those in `right` end at the bar's right edge in their order, without overlapping;
  - hiding the only shown item removes the bar and the body grows to the bottom; showing it brings the bar back; each checked after its own draw;
  - a click on a button in the status bar calls its `on_click` once;
  - a zoomed node fills the body, and the status bar is still drawn below it; zooming a status bar item is refused, and the item stays in the bar;
  - every frame rule of `title-bar` holds for `status_bar` and its slot in `window`, each checked after its own draw, the saved layout's restore included;
  - an item moved from the status bar to the title bar is drawn there, and back;
  - focus by direction from the root's card downwards reaches a status bar item, and never the frame's nodes.
- A logging test alone in its file: every row of the table. The status bar's draw line is counted over every crate's lines: exactly one per draw while it is drawn, with the exact counts, and none while it is not.
- **Existing tests:** only a test that lists every line of a run, or every component created, in order, changes: it now also has the `status_bar` node's placement and creation. A test that measured the body against the window does not change, because an empty status bar takes no room. Each change is listed when it is made.

## Test extension: `examples/window_tour`

A new case, `WINDOW_TOUR=status-bar`. It places a card as the root, a label `window_tour` in the status bar's `left`, and a button `ping` in its `right` that logs `ping`. The card has a button `hide status items` that hides both items, and then reads `show status items` and shows them again. A second case, `WINDOW_TOUR=status-items`, places only the two items, with no root, so that it can run beside `layout_tour`. Use a fresh home: `YARD_HOME=/tmp/yard-window`.

| Command and action | What to look for |
|---|---|
| `cargo run dev` | No status bar: the body reaches the bottom of the window. `window-frame` holds four placements. |
| `WINDOW_TOUR=status-bar cargo run dev` | The status bar at the bottom, `window_tour` on its left and `ping` on its right. The card's size ends above the bar. |
| Click `ping` | `ping` in the log. |
| Click `hide status items`, then `show status items` | The bar goes and the card grows to the bottom, then the bar comes back and the card shrinks. |
| `LAYOUT_TOUR=zoom WINDOW_TOUR=status-items cargo run dev`, zoom `b` | `b` fills the body, between the title bar and the status bar. |
| `{"theme": {"mode": "light"}}`, and `{"theme": {"ui_density": "compact"}}` | The bar follows the theme, and its padding the density. |

## Under review

- `yard_ui/status_bar`, new
- `yard_shell/shell`, in its dev version

## Not in this feature

- Zed's setting to hide the status bar, its right-click menu to hide an item, and its keyboard navigation between items.
- The rounded bottom corners and border of client-side decorations on Linux: with the window buttons of Linux and Windows, later.
