# window_tour

A test extension for the window engine. When `app/riso/src/dev/review.rs` lists it as `("window_tour", window_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app.

Without a case it does nothing, so a plain `cargo run dev` shows only what the app really does: the window's frame, with the title bar drawn by riso and an empty body. With a case, it registers components and places nodes through the handle the startup gives it, as any extension would.

It grows with the window engine, one case per feature. Its components are test items: a card shows its node ID and its own size, and the bar items stand for no feature.

It has no stage and is never in production.

## Cases

The case is chosen with the environment variable `WINDOW_TOUR`. An unknown case logs a WARN that names the known ones. Every case runs inside a `window-tour` group. Use a fresh home: `RISO_HOME=/tmp/riso-window`.

### Feature 1: title-bar

`WINDOW_TOUR=title-bar` places a card as the root, a label `window_tour` in the bar's `left`, and a button `ping` in the bar's `right`, which logs `ping`.

| Command and action | What to look for |
|---|---|
| `cargo run dev` | The bar, drawn by riso, with the traffic lights and `riso` on the left. `window-frame` in the trace, with the `root-place` of `window` and the `node-place` of `title_bar` and `body` inside it. |
| `WINDOW_TOUR=title-bar cargo run dev` | `window_tour` on the left instead of `riso`, and `ping` on the right. The card fills the body, below the bar. |
| Click `ping` | `ping` inside `button-click{id=ping}`, and the window does not move. |
| Drag the bar | The window moves. Double-click it: the window zooms, or minimizes, as macOS is set. |
| `LAYOUT_TOUR=zoom cargo run dev`, zoom `b` | `b` fills the body, under the bar. |
| `{"theme": {"mode": "light"}}`, and `{"theme": {"ui_font_size": 20}}` in `$RISO_HOME/config/settings.json` | The bar follows the theme, and grows with the font size: 35px at 20px. |
