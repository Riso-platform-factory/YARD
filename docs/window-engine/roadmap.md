# Window engine roadmap

## Goal

The window engine gives every window its frame: a title bar and a status bar that extensions put items in, a layer above the layout for modals and messages, and a menu bar. It also opens several windows, remembers them, and restores them.

## What is built

The startup opens one window with a transparent system title bar, and the shell places the window's frame: a title bar drawn by riso, with the slots `left`, `center`, and `right` and the product's name as the title, a body below it, and a status bar at the bottom, as Zed's, with the slots `left` and `right`, which takes no room while nothing in it is shown. Above the frame, a modal layer shows one modal at a time, as Zed's: an extension opens it with `toggle_modal`, the modal takes the focus, closes on escape, a click outside, or its own buttons, and gives the focus back, and the window is dimmed behind it only when the modal asks. An extension's root fills the body, and a zoomed node fills the body. The frame belongs to the shell: extensions put items only in the bars' slots. The application quits when the last window closes. The layout tree is kept once for the application, not per window.

## Decisions already made

- The parts of a window are ordinary nodes of the shell's own components: a title bar with the slots `left`, `center`, and `right`, a body, and a status bar with the slots `left` and `right`.
- An extension contributes to a bar by placing a component of its own in a slot of the bar. The bar draws what its slot holds and knows nothing about it.
- A part with nothing shown in it takes no room.
- The title bar shows the window's title while its `left` slot is empty.
- Every window draws its own title bar.
- Each window has its own tree.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/window-engine/<slug>.md`. The user approves the contract before any code.
- One test extension, `examples/window_tour`, grows with the features: it puts items in the bars, opens a modal, shows messages, and opens windows.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`: `TitlebarOptions`, `open_window`, `window_bounds`, `observe_window_bounds`, `prompt`, `set_menus` | Apache-2.0 | Used directly |
| `platform_title_bar`: the title bar's drawing and the window buttons of Linux and Windows, 364 lines | GPL-3.0-or-later | It cannot be a dependency: it needs `workspace`, `project`, and `settings`. It is copied into `riso_ui` and adapted |
| `workspace/src/status_bar.rs`, 506 lines | GPL-3.0-or-later | Copied into `riso_ui` and adapted |
| `workspace/src/modal_layer.rs` (331 lines), `toast_layer.rs` (268 lines), `notifications.rs` (1,979 lines) | GPL-3.0-or-later | The drawing and the focus handling of the two layers are copied into `riso_ui` and adapted |
| `workspace/src/persistence.rs`, `session` | GPL-3.0-or-later | Not UI, so not copied. They keep windows in a database. riso keeps them in JSON files |
| `title_bar`, `zed/src/zed/app_menus.rs` | GPL-3.0-or-later | Reference for what Zed puts in its title bar and its menus |

The earlier project's `crew_ui` already holds a title bar and a status bar adapted from the same files of Zed. They are ported.

## Features

Status is one of: planned, dev, production.

### The frame

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `title-bar` | The tree of a window starts with a frame of the shell's own: the title bar and the body. An extension's root goes in the body, as today, and a zoomed node fills the body. The status bar joins the frame in `status-bar`. The window draws its own title bar, with the slots `left`, `center`, and `right`. Dragging the bar moves the window, and the OS's window buttons stay | The tour places a label on the left and a button on the right. Without them, the bar shows `riso` | `platform_title_bar`. `gpui`: `TitlebarOptions` | UI kit: `gallery-and-text`, `button` | production |
| 2 | `status-bar` | The status bar, with the slots `left` and `right`. It takes no room while it holds nothing | The tour places an item on each side. Without them there is no bar | `status_bar.rs` | 1. UI kit: `divider` | production |

### Above the layout

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 3 | `modal-layer` | A layer above the tree that shows one modal at a time, over a dimmed window when the modal asks for it, as in Zed. The modal takes the focus, closes with escape or with a click outside, and gives the focus back | A button opens a modal with a line of text and a close button | `modal_layer.rs` | UI kit: `modal`. Keymap engine: `default-keys` | production |
| 4 | `toasts` | An extension shows a short message over the window: information, a warning, or an error. It goes away by itself or with a click, and can carry one button | Three buttons show the three kinds | `toast_layer.rs`, `notifications.rs` | UI kit: `button`, `icon` | dev |
| 5 | `prompts` | A question that must be answered, drawn by the OS: a message, a detail, and buttons | A button asks a question and logs the answer | `gpui`: `prompt` | none | planned |

### The menu bar

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 6 | `app-menu` | The application's menu bar. The composition root gives the menus as lists of actions. An item runs its action and shows its key | A menu with the shell's actions: choosing `Zoom` zooms the focused card | `gpui`: `set_menus` | Keymap engine: `layout-actions`, `default-keys` | planned |

### Remembered windows

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 7 | `window-bounds` | The place and the size of the window are saved when they change, and used at the next start. A screen that is gone gives the default place | Move and resize the window, quit, start: it is where it was | `gpui`: `window_bounds`, `observe_window_bounds` | Foundation 6: atomic file write | planned |
| 8 | `window-tree` | The tree, the user layer, the zoom, and the views belong to a window. The registered components stay with the application. With one window, nothing changes on screen | Every earlier case behaves as before, and the log names the window | none | Layout engine: `save-restore` | planned |
| 9 | `new-window` | An action opens another window with the default layout. Each window is rearranged on its own, and the application quits when the last one closes | Two windows, rearranged differently | `gpui`: `open_window` | 8. Keymap engine: `layout-actions` | planned |
| 10 | `restore-windows` | The windows that are open at quit are opened again at the next start, each with its place, its size, and its layout | Two windows, quit, start: both are back | `session`, as reference | 7, 9 | planned |

## Open decisions

- **One user layer per window, or one shared by all windows.** With several windows, the user's changes can belong to each window, or to the layout that the windows show. The earlier project chose the second: the user layer belongs to a named layout, and every window that shows this layout shares it, so a change in one window appears in the others. Proposed: each window has its own user layer in feature 9, and the shared one comes with the layout engine's `named-layouts`.

## Later

- Window buttons drawn by the application on Linux and Windows.
- Asking the components of a window before it closes, so that unsaved work is not lost.
- Full screen, and the window tabs of macOS.
- Dragging a node from one window to another.
- The progress of background work in the status bar.
