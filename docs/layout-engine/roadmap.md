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
| 9 | `drag-to-move` | Dragging a node to another container or to an edge moves it. The place it would land is highlighted | Dragging a tab onto another tab bar, and onto the edge of a split | drag and drop of `gpui`; drop targets and split on drop of `pane.rs` | 8 | planned |
| 10 | `zoom` | One node takes the whole window for a while, and goes back | A card zoomed and restored; the others kept their state | the zoom of `dock.rs` | 8 | planned |

### The user's layout

| # | Slug | What it adds | What the test extension shows | From Zed | Needs | Status |
|---|---|---|---|---|---|---|
| 11 | `user-layer` | What the user changed is kept apart from the default layout, as a list of operations. Resetting drops it | Rearranging, then resetting to the default | none | 9 | planned |
| 12 | `save-restore` | The user's changes are saved, and applied again at the next start. An operation whose node is gone is skipped and logged | Rearranging, quitting, starting again: the same layout | none | 11, foundation 6 | planned |

## Later

These come after feature 12. Their order is decided when they are reached: named layouts to switch between, a model shared with a subtree, a slot that makes one copy per item (as a tab per open file needs), several windows, moving between nodes with the keyboard, and an inspector of the tree.
