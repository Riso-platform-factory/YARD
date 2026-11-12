# UI kit roadmap

## Goal

The UI kit is the set of base components that everything is drawn with: text, icons, buttons, lists, inputs, menus. They are in `riso_ui`, wrapped from Zed's `ui` crate. Software built on risoapp then looks and behaves like Zed, and no crate outside `riso_ui` touches Zed's GPL code.

A base component is not a layout component. It has no node ID, it is not registered with the shell, and it holds no place in the tree. A layout component uses base components inside its own drawing.

## What is built

In production, in `riso_ui`: `Tab` and `TabBar`, with dragging and dropping, and the axis that lays out a split with its handles.

## Rules

- Every base component is a type of riso's own. No type of Zed appears in what `riso_ui` exposes.
- A component of the shell or of an extension never writes a color, a font family, or a size of text in pixels. It uses the base components and the palette. When the kit lacks a part that Zed has, the part is wrapped here. It is not imitated with plain elements.
- Before a part is wrapped, the way Zed's own views use it is read: its sizes, its spacing, its colors, its states. The wrapper keeps them.
- A base component is wrapped when a feature of another roadmap needs it, or when the user asks for it. The table gives the order in which the other roadmaps need them.

## How this roadmap is followed

- Each feature follows the workflow in `AGENTS.md` from foundation to production, and starts with a short contract in this folder, `docs/ui-kit/<slug>.md`.
- One test extension, `examples/ui_gallery`, grows with the features. It has one page per base component, as tabs. A page shows every variant and every state of its component.
- A page is checked beside the same component in Zed: the two must match in sizes, spacing, and colors.

## Reusing Zed

| From Zed | License | How it is reused |
|---|---|---|
| `ui`: about 40 components | GPL-3.0-or-later | A dependency of `riso_ui`, as today. Each component is wrapped in a type of riso's own |
| `icons`, and the icon files in `assets/icons` | The crate is GPL-3.0-or-later. The files are Lucide's, under the ISC license, and Zed's own; `assets/icons/LICENSES` says which | The crate comes with `ui`. The files that are used are copied with their license |
| `menu`: the actions of menus and lists (confirm, cancel, next, previous) | GPL-3.0-or-later | It comes with `ui` |
| `ui_input` | GPL-3.0-or-later | Not used. Its input field needs an editor given from outside, which in Zed is the `editor` crate |
| `gpui/examples/input.rs`, a text input in 920 lines | Apache-2.0 | Adapted into the kit's text input |
| `component`, `component_preview` | GPL-3.0-or-later | Reference for the gallery |

The earlier project's `crew_ui` wraps most of these under the same rules: buttons, lists, icons, text, callouts, a text input, and a code text. Its code is ported.

## Features

Status is one of: planned, dev, production.

| # | Slug | What it adds | What the gallery shows | From Zed | Needs | First needed by | Status |
|---|---|---|---|---|---|---|---|
| 1 | `gallery-and-text` | The gallery itself, and the text components: a label and a headline in Zed's sizes and colors, and the row and column helpers | A page with every size and color of text | `Label`, `Headline` | none | Window engine: `title-bar` | planned |
| 2 | `icon` | An icon by name, in Zed's sizes and colors. The set holds the icons the other roadmaps use, and grows by need. The startup gives GPUI the bundled files | A page with every bundled icon and its name | `Icon`, `IconName`, `assets/icons` | 1 | Window engine: `toasts` | planned |
| 3 | `button` | A button with a label, an icon, or both, in Zed's styles and sizes. It can be disabled or toggled, and it tells its owner when it is clicked | A page with every style and state. A click is logged | `Button`, `IconButton` | 2 | Window engine: `title-bar` | planned |
| 4 | `divider` | A thin line between groups, horizontal or vertical | A page with both | `Divider` | 1 | Window engine: `status-bar` | planned |
| 5 | `modal` | The frame of a modal: a header, sections, and a footer with buttons | A page with a modal drawn in place | `Modal` | 3 | Window engine: `modal-layer` | planned |
| 6 | `list` | A list with items that can be selected or disabled, with a place for an icon before and after the text, headers, and separators | A page with a list in each state | `List`, `ListItem`, `ListHeader` | 2 | Theme engine: `theme-list` | planned |
| 7 | `text-input` | An input of one line: typing, selecting, the clipboard, input methods, a placeholder. It tells its owner when the text changes and when it is confirmed | A page where text is typed, selected, and pasted | `gpui/examples/input.rs` | 1 | Keymap engine: `command-palette` | planned |
| 8 | `scroll` | An area that scrolls, with Zed's scrollbar, also for lists too long to draw whole | A list of a thousand items | `Scrollbar`. `gpui`: `uniform_list` | 6 | Keymap engine: `command-palette` | planned |
| 9 | `key-hint` | The keys bound to an action, drawn as Zed draws them | A page with the keys of the shell's actions | `KeyBinding` | 1. Keymap engine: `default-keys` | Keymap engine: `command-palette` | planned |
| 10 | `tooltip` | A short text shown when the pointer rests on an element | Tooltips on the buttons of the button page | `Tooltip` | 3 | Layout engine: `close-tab` | planned |

### By need

These are wrapped when something first needs them. Nothing planned needs them yet.

| Slug | What it adds | From Zed |
|---|---|---|
| `context-menu` | A menu of entries, opened with the right button or from a button | `ContextMenu`, `PopoverMenu`, the right-click menu |
| `toggle` | A checkbox and a switch | `Checkbox`, `Switch` |
| `callout` | A message inside a view, with a severity | `Callout`, `Banner` |
| `tree-item` | An item of a tree that opens and closes, with indent guides | `TreeViewItem`, `Disclosure`, `IndentGuides` |
| `code-text` | Read-only text in the code font. It is the first user of the theme engine's `code-font` | none: riso's own, on GPUI's text |
| `text-area` | An input of several lines, with wrapping, scrolling, and undo | `gpui/examples/input.rs` |
| `markdown` | Markdown drawn as text | none: riso's own |
| `data-table`, `progress` | A table of rows and columns, and a progress bar | Zed's data table and progress components |
