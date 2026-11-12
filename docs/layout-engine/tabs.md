# Layout feature 6: tabs, with foundation 7: tab widgets

## What it adds

A second container. A tabs container shows one child at a time, under a tab bar with one tab per child. Clicking a tab shows its child. The children that are not shown keep their views and their state.

The tab bar is drawn with Zed's own `Tab` and `TabBar`, from its `ui` crate, wrapped in `riso_ui`. That is foundation 7. It is built here, with its first user, as the foundations roadmap says.

## Foundation 7: tab widgets in `riso_ui`

- `riso_ui` depends on Zed's `ui` crate for the first time. `ui` is GPL, so it stays inside `riso_ui`, as `theme` does. The GPL boundary test checks that no other crate depends on it.
- **`riso_ui` exposes its own two types, wrapping Zed's:**
  - `Tab`: an ID, a label, whether it is selected, and what happens when it is clicked;
  - `TabBar`: an ID and its tabs.

  The tab bar works out each tab's position (first, last, or before or after the selected one) the way Zed's `pane.rs` does, so the borders between tabs look like Zed's. Nothing of Zed's types is exposed.
- **Zed's `ui` components read the theme's settings through a provider:** the fonts, their sizes, and the density.
  - `riso_ui`'s `init` installs one, with the fixed values `riso_ui` already has: IBM Plex Sans at 16px, and the default density.
  - The code font is not bundled yet, so the provider gives the UI font for code too. This is marked `TODO(code-font)`.
  - This is a change to `riso_ui/theming`, which is in production. It is made in place and stamped again when you approve it, as with the `text` and `border` colors.

## Model

- The shell provides a third component type of its own, `riso.tabs`, with one slot, `children`. Like the splits, any extension may place it.
- Each tab's label is its child's node ID. Labels of their own come with props, later.
- At first, the first child is selected. The selection lives as long as the window, and is not saved.
- A tabs container with no children shows an empty tab bar.

## Logging

| When | Level | Line, in `riso_shell` |
|---|---|---|
| A tabs container is created | `debug` | `tabs created`, with its number of tabs, inside its `component-create` |
| A tab is clicked | `debug` | `tab selected`, with the container's node ID and the tab's node ID |

Clicking the tab that is already selected changes nothing, and logs nothing.

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/tabs`, dev, new | `Tab` and `TabBar`, wrapping Zed's |
| `riso_ui/theming`, production | `init` installs the settings provider that Zed's `ui` needs |
| `riso_shell/shell` | A dev version beside the production one. It adds `riso.tabs`: a tab bar over the selected child. |
| `riso/startup` | A dev copy that uses the dev shell |

## Test extension: `examples/layout_tour`

It is loaded again, with the guest.

| Command and action | What to look for |
|---|---|
| `LAYOUT_TOUR=tabs cargo run dev` | A tab bar with `a`, `b`, `c`, drawn as Zed draws its tabs, with `a` selected and card `a` under it. |
| Same, click `b`, then `c`, then `a` | Each click shows its card, and logs `tab selected`. Card `a` comes back with the same view; its size label is still right. |
| `LAYOUT_TOUR=tabs-in-split cargo run dev` | A row: card `a` on the left, and on the right a tabs container with `b` and `c`. Dragging the row's line resizes the tabs container too. |
| `LAYOUT_TOUR=empty-tabs cargo run dev` | An empty tab bar over the background. |
| `LAYOUT_TOUR=tabs LAYOUT_GUEST=tab cargo run dev` | The guest's card is a fourth tab, `guest`, at the end. |

## Under review

- `riso_ui/tabs`
- `riso_ui/theming`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- Closing a tab.
- Dragging tabs: feature 9.
- Tab icons, labels other than node IDs, and saving the selection.
- Scrolling an overflowing tab bar.
