# Layout engine roadmap

## Goal

The layout engine is how software built on risoapp puts its user interface together: which components are on screen, where each one is, who contributed it, and how the user rearranges them. An extension registers components and places them; the application does not know the extension.

GPUI already draws and sizes elements. The layout engine does not replace that. It is the model above it.

## How this roadmap is followed

- Features are built in the order of the table, one at a time. Each one is small, and follows the workflow in `AGENTS.md` from foundation to production.
- A feature starts with a short contract in this folder, `docs/layout-engine/<slug>.md`: what it adds, the rules it keeps, and what the test extension shows. The user approves the contract before any code.
- One test extension, `examples/layout_tour`, grows with the features: one case per feature, chosen by its parameter. Its components are test cards: a card shows its own node ID and size, and stands for no feature.
- Nothing about any application goes into the engine.

## Reusing Zed

Zed is reused first, and only through the wrapper crate `riso_ui`, so that the license can change later.

| From Zed | License | How it is reused |
|---|---|---|
| `gpui`, `gpui_platform` | Apache-2.0 | Used directly, anywhere: elements, flex layout, drag and drop, focus, actions |
| `ui`, `theme` | GPL-3.0-or-later | As dependencies of `riso_ui` only, wrapped in riso's own types |
| `workspace`: the split tree and its resize handles (`pane_group.rs`), tab dragging and drop targets (`pane.rs`), docks and zoom (`dock.rs`) | GPL-3.0-or-later | It cannot be a dependency: it needs 46 other crates of Zed, among them the project, the language servers, and the collaboration client. The pieces needed are copied into `riso_ui` and adapted, each file keeping Zed's copyright and naming the file it comes from |

The model of the layout (the tree, the slots, the operations, the rules) is riso's own code, in a crate without GPUI. Zed has no such model to take: its workspace is written for fixed docks around panes.

## Features

Status is one of: planned, dev, production.

### A tree on screen

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 1 | `component-registry` | An extension registers a component type: an ID and a way to create its view. The window shows one root node | One test card fills the window | `gpui` | foundations 4 and 5 | production |
| 2 | `slots` | A component declares named slots. A layout places nodes in slots. Rules: IDs are unique, the component and the slot exist, there is no cycle | A card inside a card. A layout that breaks a rule is refused, and the log says why | `gpui` | 1 | production |
| 3 | `extension-contribution` | An extension places a node in a slot of a component it does not own | A second extension's card appears inside the first one's slot. Without that extension, the slot is empty | `gpui` | 2 | production |

### Containers

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 4 | `split` | A container that shows its children side by side, in a row or a column, in equal shares. Splits nest | Three cards in a row; a column inside a row | the axis layout of `pane_group.rs` | 3 | production |
| 5 | `split-resize` | Dragging the handle between two children changes their shares. A child has a minimum size | Dragging a handle; a child that stops at its minimum | the handles and the resize rules of `pane_group.rs` | 4 | production |
| 6 | `tabs` | A container that shows one child at a time under a tab bar | Three cards as tabs; clicking a tab shows its card | `TabBar` and `Tab` of `ui` | 4, foundation 7 | production |
| 7 | `hide-show` | A hidden node keeps its state and gives its space to the others. An action hides and shows a node | A card that counts; hidden and shown again, it kept its count | the toggling of `dock.rs`, as reference | 4 | production |

### Changes while the application runs

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 8 | `operations` | The tree changes only through operations: add, remove, move, hide, show. The engine checks each against the rules; a refused one is logged, not applied | Buttons on a card that add, move, and remove cards; one that asks for a forbidden move | none | 7 | production |
| 9 | `drag-to-move` | Dragging a node to another container or to an edge moves it. The place it would land is highlighted | Dragging a tab onto another tab bar, and onto the edge of a split | drag and drop of `gpui`; drop targets and split on drop of `pane.rs` | 8 | production |
| 10 | `zoom` | One node takes the whole window for a while, and goes back | A card zoomed and restored; the others kept their state | the zoom of `dock.rs` | 8 | production |

### The user's layout

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 11 | `user-layer` | What the user changed is kept apart from the default layout, as a list of operations. Resetting drops it | Rearranging, then resetting to the default | none | 9 | production |
| 12 | `save-restore` | The user's changes are saved, and applied again at the next start. An operation whose node is gone is skipped and logged | Rearranging, quitting, starting again: the same layout | none | 11, foundation 6 | production |

### Nodes with data

Features 13 and later are the second part of the engine. Where the column `Needs` names a feature of another roadmap, the feature waits for it.

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 13 | `props` | A node carries props: a small configuration given when it is placed, typed by its component, with a default for every value. Props that cannot be read are logged, and the defaults are used. A new operation changes some of a node's props while the application runs. The view is told, and the operation is recorded and saved like the others | A card's title and tint come from its props. A button changes them, and they survive a restart | none | 12 | production |
| 14 | `titles` | A node has a title, and optionally an icon, that its component gives. A tab shows the title instead of the node ID | Tabs named `Alpha` and `Beta`, one of them with an icon | the content of `ui`'s `Tab` | 13. UI kit: `icon` | production |
| 15 | `required-nodes` | A component can be declared required. Its nodes cannot be hidden or removed, and neither can a node above them | Hiding or removing the required card is refused with an `INFO` | none | 8 | production |
| 16 | `slot-count` | A slot says whether it holds one node or many. A second node in a slot for one is refused | A card with a slot for one: the second placement is refused with an `INFO` | none | 2 | production |
| 17 | `placement-order` | A placement or a move can say first, or before or after a named node, instead of a position by number | An item placed before the item of another extension | none | 8 | production |
| 18 | `contexts` | A component gives a model to everything under its node. A component below asks for it by its type and receives it. A node cannot be placed or moved to a place where a model it requires is missing. An extension can also give a model to a whole window | Two cards under one counter show the same count. Moving one of them out of the counter is refused | none | 13 | production |

### Sizes

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 19 | `sizes-as-props` | The shares of a split are props of the split, so a resize is an operation: recorded, saved, restored, and dropped by a reset. The steps of one drag join into one operation | Drag a line, quit, start: the same shares. Reset: equal shares again | none | 13 | production |
| 20 | `fixed-sizes` | A child of a split is flexible, with a share, or fixed, with a size. When the split grows, only the flexible children grow. When it shrinks, they shrink first | A side card keeps its width while the window is resized | the resizing of `dock.rs`, as reference | 19 | production |
| 21 | `component-sizes` | A component declares a preferred and a minimum size, in rems, so that they follow the size of the UI font. The minimum of a container comes from its children. This replaces the fixed minimums of 80px and 100px | A card with a larger minimum stops the drag earlier, and a larger UI font moves the limit | none | 20. Theme engine: `ui-font` | dev |
| 22 | `snap-hide` | Dragging a child below its minimum hides it, as a `hide` operation. Dragging the line back shows it | A side card dragged shut disappears, and comes back when the line is dragged out | none | 21 | planned |
| 23 | `empty-nodes` | A component can say that it is empty now. A container gives an empty child no room, and is itself empty when nothing in its slots is shown | A column of two cards: one says it is empty, and the other takes its room | none | 7 | planned |

### Tabs and drops, completed

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 24 | `close-tab` | A tab has a close button. Closing removes the node. The tab of a required node has no button | Closing a tab removes its card, and `view dropped` is logged | the end slot of `ui`'s `Tab` | 15. UI kit: `icon`, `button`, `tooltip` | planned |
| 25 | `tab-overflow` | A tab bar scrolls when its tabs do not fit, and selecting a tab scrolls it into view | Twenty tabs in a narrow window | the scrolling of `ui`'s `TabBar` | 6 | planned |
| 26 | `split-on-drop` | Dropping a tab on any edge of a container puts a new split around that container, with the node in a new tabs container beside it. Dropping on the center of a container moves the node into it | A tab dropped on the top edge of a card becomes a row above the card | split on drop of `pane.rs` | 9, 13 | planned |
| 27 | `toggle` | A component of the shell: a button that shows or hides the node that its props name. With `alone`, showing that node hides the others in its slot | Two buttons in the status bar show and hide two side cards, one at a time | the panel buttons of Zed's status bar, as reference | 13. UI kit: `button`, `icon`. Window engine: `status-bar` | planned |

### Content that comes and goes

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 28 | `item-slots` | A slot that makes one copy of its nodes for each item of a model, such as each open document. Each copy gets its item as a context. Items come and go without layout operations | A model with a list of names, and a button to add and remove one: there is one card per name | none | 18 | planned |
| 29 | `item-tabs` | A component of the shell: a tab bar over an item slot, with one tab per item, a close button on each tab, and a button to add an item. The buttons tell the model | Tabs that follow the list of names | `pane.rs`, as reference | 24, 28 | planned |

### Named layouts

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 30 | `named-layouts` | An extension registers a named layout: a list of operations over the default layout. A window shows one named layout at a time and can switch to another. Nodes with the same ID keep their views, and each named layout has its own user layer and its own file | Switching between two arrangements of the same cards keeps their counts | none | 12 | planned |

### Tools

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 31 | `inspector` | A test extension that shows the tree: each node with its component, its extension, its props, whether it is hidden, and whether the default layout or the user layer put it there | The inspector beside any case of the layout tour | none | 13. UI kit: `list` | planned |

## Later

These are not planned yet: undoing one operation, replacing a node's component, wrapping a node in another component, an edit mode that shows outlines and slots, a container that stacks its children as sections that open and close, limits on which extension may change which node, reading a saved layout of another version, a layout per project folder, dragging nodes that are not tabs, and animations.

Three things that the contracts of features 1 to 12 left out are in other roadmaps: several windows are in the window engine's, moving the focus and zooming with the keyboard are in the keymap engine's, and saving the selected tab and the zoom is in the state engine's.
