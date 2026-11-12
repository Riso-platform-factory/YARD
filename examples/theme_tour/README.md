# theme_tour

A test extension for the theme engine, in `riso_ui`. When `app/riso/src/dev/review.rs` lists it as `("theme_tour", theme_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the theme engine again, or when the next theme feature is under review.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does. With `THEME_TOUR=card`, it places one card as the root. The card shows the active theme's name, then each color of the palette as a swatch with its name, then a line in the UI font. A change of theme can then be seen at a glance.

Use a fresh home, so that the user's real files are never touched: `RISO_HOME=/tmp/riso-theme`. The tour writes no file: write `config/settings.json` under the fresh home yourself.

## Feature 1: theme-setting

| File `config/settings.json` | `THEME_TOUR=card cargo run dev` shows |
|---|---|
| none | `One Dark`, a dark card. `theme-apply{mode=dark, theme=One Dark}` inside `app-init` with `applied`. |
| `{"theme": {"mode": "light"}}` | `One Light`: a light card and dark text. `theme=One Light`. |
| `{"theme": {"mode": "dark", "dark": "One Light"}}` | `One Light`. |
| `{"theme": {"mode": "light", "light": "Solarized"}}` | `One Dark`, and the `WARN` `theme Solarized is not registered`. |
| `{"theme": {"mode": "dim"}}` | `One Dark`, and the settings engine's `WARN` that the section `theme` cannot be read. |
| `THEME_TOUR=other` | A `WARN` that the case is unknown, and an empty window. |

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
