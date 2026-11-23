# Themes

YARD's theme engine controls colors, the UI font, and spacing density across every window. All appearance options live in the `"theme"` section of [`settings.json`](./settings.md) and update live as soon as the file changes.

## Theme Configuration

```json
{
  "theme": {
    "mode": "system",
    "dark": "One Dark",
    "light": "One Light",
    "ui_font_family": "IBM Plex Sans",
    "ui_font_size": 16,
    "ui_density": "default"
  }
}
```

| Key | Values | Default |
|---|---|---|
| `mode` | `"system"`, `"dark"`, `"light"` | `"system"` |
| `dark` | Name of a registered theme used in dark mode | `"One Dark"` |
| `light` | Name of a registered theme used in light mode | `"One Light"` |
| `ui_font_family` | Installed or bundled font family name | `"IBM Plex Sans"` |
| `ui_font_size` | Number in pixels, clamped to `6.0..=100.0` | `16` |
| `ui_density` | `"compact"`, `"default"`, `"comfortable"` | `"default"` |

## System Appearance and Choosing a Theme

- When `mode` is `"system"`, YARD selects the theme named by `dark` or `light` to match the operating system's appearance and switches automatically whenever the OS appearance changes.
- When `mode` is `"dark"` or `"light"`, YARD always uses the corresponding theme regardless of OS appearance.
- If a configured theme name is not registered, YARD logs a `WARN` and keeps the currently active theme.
- When a UI picker chooses a theme via `yard_ui::choose_theme`:
  - It writes the chosen theme name to `dark` (for a dark theme) or `light` (for a light theme) in `settings.json`.
  - If `mode` is `"system"` and the chosen theme matches the current OS appearance, `mode` stays `"system"`; otherwise `mode` switches to `"dark"` or `"light"` so the choice takes effect immediately.

## Bundled and User Themes

YARD bundles Zed's **One Dark** and **One Light** themes.

To add custom themes, place `.json` files in Zed's theme family format directly inside `<config>/themes/` (see [Product Config](./building/product-config.md#application-directories)). Each file is a JSON object with `name`, `author`, and a `themes` array, where each entry has `name`, `appearance` (`"light"` or `"dark"`), and `style`.

- Only `.json` files directly inside `<config>/themes/` are scanned (subdirectories and non-`.json` files are ignored). A missing `themes/` directory is normal.
- User theme files are loaded once at startup in sorted filename order.
- A user theme with the same name as a bundled theme replaces it; when two user files define the same theme name, the later file in sorted order wins.
- Each unreadable or invalid theme file logs a `WARN` and is skipped without affecting other theme files.
- Restart the application to pick up newly added or edited user theme files.

## UI Font

YARD bundles **IBM Plex Sans** (Regular, Italic, SemiBold, and SemiBoldItalic) as the default UI font at `16px`.

- Changing `ui_font_family` switches all UI text to the requested font family. If the family is not available, YARD logs a `WARN` and keeps `"IBM Plex Sans"`.
- Changing `ui_font_size` scales both text and every layout size defined in `rem` units (including title bar height, component minimums, and preferred sizes). Values outside `6.0..=100.0` are clamped with a `WARN`.

## UI Density

`ui_density` adjusts spacing across UI kit components (such as status bar padding, tab bars, buttons, and lists):

- `"compact"`: tighter spacing.
- `"default"`: standard Zed spacing.
- `"comfortable"`: looser spacing.

## Reference

### Lifecycle Trace Groups

| Group | Fields | When |
|---|---|---|
| `themes-load` | `path` | Loading user theme files from `<config>/themes/` at startup |
| `theme-apply` | `mode`, `theme` | Applying the active theme on startup or change |
| `appearance-change` | `appearance` | Responding to an operating system light/dark appearance change |
| `ui-font-apply` | `family`, `size` | Applying UI font family and size |
| `density-apply` | `density` | Applying UI density |
| `theme-choose` | `name` | Selecting a theme via `yard_ui::choose_theme` |
