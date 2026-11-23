# UI Kit

`yard_ui` is YARD's base component library and theme boundary. It wraps Zed's `ui` and `theme` crates behind YARD's own types so no other crate in the workspace depends on Zed's GPL crates (enforced by the `gpl_boundary` test).

Components in extensions and applications should draw using `yard_ui` components, `yard_ui::palette(cx)`, and `rem` units rather than hardcoding pixel font sizes or colors.

## Layout Helpers and Text

- **Layout helpers:** `h_flex()` and `v_flex()` return plain GPUI flex containers (a centered horizontal row and a vertical column).
- **Text:** `Label::new(text)` (`LabelSize`: `Large` `1rem`, `Default` `0.875rem`, `Small` `0.75rem`, `XSmall` `0.625rem`) and `Headline::new(text)` (`HeadlineSize`: `XSmall` `0.88rem`, `Small` `1rem`, `Medium` `1.125rem` default, `Large` `1.27rem`, `XLarge` `1.43rem`).
- **Colors:** `TextColor` maps semantic roles to the active theme (`Default`, `Muted`, `Accent`, `Disabled`, `Placeholder`, `Error`, `Warning`, `Success`).
- **Code font fallback:** `yard_ui`'s `ThemeSettingsProvider` currently supplies the active UI font family for `buffer_font` and a fixed `15px` `buffer_font_size`. A dedicated code font setting is tracked on the [Roadmap](../development/roadmap.md).

## Component Guides

| Page | Covers |
|---|---|
| [Buttons, Icons, and Dividers](./ui-kit/buttons-and-icons.md) | `Button`, `IconButton`, `ElevationIndex`, `Icon`, `IconName`, `Assets`, `Divider` |
| [Inputs, Lists, and Scrolling](./ui-kit/inputs-and-lists.md) | `TextInput`, `List`, `ListItem`, `ScrollArea`, `ScrollList` |
| [Overlays, Key Hints, and Tooltips](./ui-kit/overlays-and-hints.md) | `Modal`, `ModalSurface`, `KeyHint`, `key_hint_text`, `Tooltip`, `Toast` |

## Shell and Window Primitives

`yard_ui` also provides the low-level widgets used by `yard_shell`:

- **Tabs:** `Tab`, `TabBar`, `tab_bar_height(cx)` — supports icons, hover close buttons (`on_close`), middle-click close, drag-and-drop, horizontal scrolling (`track_scroll`), and trailing end children (`end_child`).
- **Splits:** `axis`, `Axis`, `AxisChild`, `AxisSize` (`Flex`, `Fixed`), `Resized` (`Dragged`, `Reset`) — handles flexible/fixed sizing, per-child minimums, double-click reset, and snap-hide/snap-open.
- **Window chrome:** `TitleBar`, `titlebar_options`, `StatusBar`, `WindowButtons`, `WindowsWindowButtons`, `WindowControlAction`, `default_button_layout`, `parse_button_layout`, `client_decorations`, `window_background`.

## Theme Helpers

- `yard_ui::palette(cx) -> Palette`: exposes `background`, `text`, `border`, and `drop_target` colors from the active theme.
- `yard_ui::ui_font(cx)` and `yard_ui::ui_font_size(cx)`: current UI font and size.
- `yard_ui::active_theme(cx) -> String`: name of the currently active theme.
- `yard_ui::theme_list(cx) -> Vec<ThemeEntry>`: all registered themes (`ThemeAppearance::Dark` first, then `Light`, sorted alphabetically).
- `yard_ui::load_user_themes(cx, dir) -> usize`: loads custom theme files from `dir` (normally `Paths::themes_dir()`) and returns the number of registered themes. See [Themes](../themes.md#bundled-and-user-themes) for file format and collision rules.
- `yard_ui::choose_theme(cx, name)`: writes the chosen theme into `settings.json` and updates `mode` when needed. If `name` is not registered, it logs an `ERROR` and writes nothing.

## Component Gallery (`examples/ui_gallery`)

`examples/ui_gallery` is a test extension that displays every base component and its variants across 11 tabs (`text`, `icon`, `button`, `divider`, `modal`, `list`, `text-input`, `scroll`, `key-hint`, `tooltip`, and `window-controls`).

When `ui_gallery` is enabled in `app/yard/src/dev/review.rs`, run it with:

```sh
YARD_HOME=/tmp/yard-gallery UI_GALLERY=all cargo run dev
```

Without `UI_GALLERY` set, the extension places nothing.
