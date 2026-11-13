# ui_gallery

A test extension for the UI kit, the base components in `riso_ui`. When `app/riso/src/dev/review.rs` lists it as `("ui_gallery", ui_gallery::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back to check the UI kit again, or when the next UI kit feature is under review.

Without `UI_GALLERY` it does nothing, so a plain `cargo run dev` shows only what the app really does. With `UI_GALLERY=all`, it places a tabs container as the root, with one tab per page and one page per base component. A page shows every variant of its component, each naming itself, in groups with an upper-case muted title beside a hairline, as Zed's component preview does.

Use a fresh home, so that the user's real files are never touched: `RISO_HOME=/tmp/riso-gallery`.

## Feature 1: gallery-and-text

| Command | What to look for |
|---|---|
| `cargo run dev` | Nothing from the gallery. |
| `UI_GALLERY=all cargo run dev` | A tab `text`. Its page shows a headline per size, a label per size, a label per color, a row of three labels, and a column of three. `gallery-build{pages}`, with a `page placed` line per page. |
| The same, with `{"theme": {"mode": "light"}}` in `config/settings.json` | The same page in One Light: every color follows the theme. |
| The same, with `{"theme": {"ui_font_size": 20}}` | Every text larger, in the same proportions. |
| `UI_GALLERY=other cargo run dev` | A `WARN` that the case is unknown, and an empty window. |

## Feature 2: icon

| Command | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, then the tab `icon` | Under `SIZE`, a row per size with every icon; under `COLOR`, `Check` in each text color; under `NAME`, every icon beside its name. `gallery-build{pages=2}`. |
| The same, in One Light | The icons follow the theme's colors. |

## Feature 3: button

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, the tab `button` | Under `STYLE`, `SIZE`, `ICON`, and `STATE`, every style, size, icon, and state, as in Zed. `gallery-build{pages=3}`. |
| Hover and press a button | Its hover and pressed colors. |
| Click `clicked 0 times` twice | `clicked 2 times`. Two `button-click{id=count-clicks}` groups, each with `button clicked {count}` inside it. |
| Click the check under `CLICK` | It turns accent, with `button-click{id=toggle-clicks}`, then back on the next click. |
| Click the disabled buttons | Nothing, and no `button-click`. |
| With `{"theme": {"ui_density": "compact"}}` | The buttons get smaller. |

## Feature 4: divider

| Command and action | What to look for |
|---|---|
| `UI_GALLERY=all cargo run dev`, the tab `divider` | Under `HORIZONTAL`, a solid, a dashed, and an inset divider in each color; under `VERTICAL`, labels separated by solid and dashed dividers. `gallery-build{pages=4}`. |
| The same, in One Light | The lines follow the theme. |

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`, and the app runs as it should without it.
