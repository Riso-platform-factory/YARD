# theme_tour

A test extension for the theme engine, in `riso_ui`. When `app/riso/src/dev/review.rs` lists it as `("theme_tour", theme_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the theme engine again, or when the next theme feature is under review.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With `THEME_TOUR=card`, it places one card as the root. The card shows the active theme's name, then each color of the palette as a swatch with its name, then a line in the UI font. A change of theme can then be seen at a glance.

Use a fresh home, so that the user's real files are never touched: `RISO_HOME=/tmp/riso-theme`.

The default mode is `system`. So the rows below that rely on the default, such as no file, a broken section, or a deleted file, show `One Dark` on a Mac in dark mode and `One Light` on a Mac in light mode. The tour writes no file: write `config/settings.json` under the fresh home yourself.

## Feature 1: theme-setting

| File `config/settings.json` | `THEME_TOUR=card cargo run dev` shows |
|---|---|
| none | `One Dark`, a dark card. `theme-apply{mode=dark, theme=One Dark}` inside `app-init` with `applied`. |
| `{"theme": {"mode": "light"}}` | `One Light`: a light card and dark text. `theme=One Light`. |
| `{"theme": {"mode": "dark", "dark": "One Light"}}` | `One Light`. |
| `{"theme": {"mode": "light", "light": "Solarized"}}` | `One Dark`, and the `WARN` `theme Solarized is not registered`. |
| `{"theme": {"mode": "dim"}}` | `One Dark`, and the settings engine's `WARN` that the section `theme` cannot be read. |
| `THEME_TOUR=other` | A `WARN` that the case is unknown, and an empty window. |

## Feature 2: theme-switch

Start `THEME_TOUR=card cargo run dev` with a fresh home and no file, then edit `config/settings.json` while it runs:

| Edit | What to look for |
|---|---|
| Write `{"theme": {"mode": "light"}}` | Within a second, the card turns light and says `One Light`. `theme-apply` with `applied`, inside `settings-watch`. |
| Change it to `{"theme": {"mode": "light", "dark": "One Light"}}` | Nothing changes. `unchanged`. |
| Write `{"theme": {"mode": "light", "light": "Solarized"}}` | The card stays as it was. The `WARN` `theme Solarized is not registered`. |
| Delete the file | The card turns dark again: `One Dark`. |

## Feature 3: system-appearance

Without a section, the mode is now `system`: the theme follows macOS. Start `THEME_TOUR=card cargo run dev` with a fresh home and no file:

| Action | What to look for |
|---|---|
| Start with macOS in dark mode | `One Dark`. `theme-apply{mode=system, theme=One Dark}`, then, when the window opens, `appearance-change{appearance=dark}` with `unchanged`. |
| Start with macOS in light mode | `One Light`. |
| Switch macOS between light and dark in System Settings, Appearance, while it runs | Within a moment the card follows. `appearance-change` with `theme-apply` and `applied` inside it. |
| Write `{"theme": {"mode": "light"}}`, then switch macOS | The card stays `One Light`. `mode is not system`. |

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
