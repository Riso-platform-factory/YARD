# Layout feature 14: titles

## What it adds

A node has a title, and optionally an icon, that its component gives. A tab shows its node's title and icon instead of the node ID. The title can change while the application runs, for example when the node's props change, and the tab follows.

This is Zed's `Item::tab_content_text` and `Item::tab_icon`: the item, not the pane, says what its tab shows.

## How it works

- **A component gives titles through its view.** The view implements `Titled`: `title` returns the text, and `icon` returns an icon or none (the default). The component says so once, with `Extension::register_title::<V>(cx, component)`, where `V` is the type of its views. A component that does not say so has no title, and its tab shows the node ID, as today.
- **The rule.** `register_title` is refused with an `ERROR`, a bug of the extension, when the component ID is invalid, is not in the extension's namespace, or already gives titles. It can be called before or after the component is registered.
- **Reading a title.** A tabs container reads its children's titles each time it is drawn, from their views. A title therefore follows the view's state: when the view changes its title and notifies, the tab shows the new one.
- **A view of another type.** If a node's view is not a `V`, which is a bug of the extension, the tab shows the node ID, and an `ERROR` is logged once per node.
- **The tab.** As in Zed's `Pane::render_tab`:
  - the content is a row with `gap_1`: the icon, if there is one, then the title;
  - the icon is small and muted (`IconSize::Small`, `Color::Muted`);
  - the title is a single-line label, in the default color when the tab is selected and muted otherwise, as Zed's `TabContentParams::text_color` gives for a focused pane;
  - the tab's ID stays the node ID, so selecting, dragging, and dropping are unchanged;
  - a dragged tab shows its title too.

## What code can do

```rust
// riso_shell::dev
pub trait Titled: gpui::Render {
    fn title(&self, cx: &App) -> SharedString;
    fn icon(&self, cx: &App) -> Option<riso_ui::IconName> { None }
}
impl Extension {
    pub fn register_title<V: Titled + 'static>(&self, cx: &mut App, component: &str);
}

// riso_ui (production, in place)
impl Tab {
    pub fn icon(self, icon: IconName) -> Self;
}
```

The dev shell has every other item of the production shell, unchanged.

## Logging

| Situation | Log |
|---|---|
| A component gives titles | A group `title-register{component}`, with `debug` `gives titles`; an `ERROR` for a refusal |
| A tab reads a title | `trace` `tab title {node, title}`: tabs are drawn on every frame |
| A node's view is not the type its component named | `ERROR` once per node in the diagnostic log, `view of node {node} is not the type that gives its title`; the lifecycle trace has it at `trace` each frame |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/tabs` (production, in place) | `Tab::icon`, and the tab's content drawn as Zed's: the icon and the title in a row, the title colored by selection. Stamped again. |
| `riso_shell/shell` dev version | `Titled`, `register_title`, and tabs that read titles |
| `riso/startup` | Dev copy that uses the dev shell. While `titles` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **UI kit, GPUI tests:** a tab with an icon is wider than the same tab without; the icon is drawn before the label (`debug_bounds`).
- **Shell, GPUI tests, with the dev shell:**
  - a tab of a node whose component gives titles shows the title, and is as wide as a tab of a node whose ID is that title;
  - a tab of a node whose component does not give titles shows the node ID;
  - a title that changes (the view changes its state and notifies) is shown on the next frame;
  - a node whose view is of another type shows its node ID, and the `ERROR` is logged once though the tab is drawn many times;
  - `register_title` before and after `register` both work;
  - `register_title` refused for another extension's component, an invalid ID, and a second time, each with an `ERROR`;
  - selecting, dragging, and dropping a titled tab work as before (the tab's ID is the node ID).
- **Logging**, alone in its file: `title-register{component}` with `gives titles`; the `ERROR` of a wrong view type once.

## Test extension: `examples/layout_tour`

A new case, `titles`. It uses the card `layout_tour.tinted` of feature 13, which now gives its title from its `title` prop, and an icon from a new prop, `icon` (`none`, `info`, or `warning`; default `none`). The layout is the controls, then a tabs container with `alpha` (`title: Alpha`, `icon: info`), `beta` (`title: Beta`), and `gamma`, a plain card of the tour that gives no title.

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-titles LAYOUT_TOUR=titles cargo run dev` | Tabs `ⓘ Alpha`, `Beta`, and `gamma`. The selected tab's title in the default color, the others muted. `title-register{component=layout_tour.tinted}` with `gives titles`. |
| Click `rename beta` | The tab reads `Bravo`. |
| Drag tab `Alpha` after `gamma` | It moves, with its title and icon. |
| Beside Zed's tabs (open two files) | The icon, its gap, and the label match in size and color. |
| `reset layout` | `Alpha`, `Beta`, `gamma` again. |

## Under review

- `riso_ui/tabs`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- A tab's close button: feature 24, `close-tab`.
- Tooltips on tabs: with `close-tab`.
- A title shown elsewhere than in a tab, such as the window's title: when a feature needs it.
- Zed's detail, which tells apart two tabs with the same title.
