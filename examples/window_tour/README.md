# window_tour

A test extension for the window engine. When `app/riso/src/dev/review.rs` lists it as `("window_tour", window_tour::init)` among the extensions it passes to the startup, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it into the running app. Every feature it covers is in production, so it is not loaded now; add both back when the next window engine feature is under review.
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

### Feature 2: status-bar

`WINDOW_TOUR=status-bar` places a card as the root, a label `window_tour` in the status bar's `left`, and a button `ping` in its `right`, which logs `ping`. The card has a button `hide status items`, which hides both items and then reads `show status items`. `WINDOW_TOUR=status-items` places only the two items, with no root, to run beside `layout_tour`.

| Command and action | What to look for |
|---|---|
| `cargo run dev` | No status bar: the body reaches the bottom of the window. `window-frame` holds four placements. |
| `WINDOW_TOUR=status-bar cargo run dev` | The status bar at the bottom, `window_tour` on its left and `ping` on its right. The card's size ends above the bar. |
| Click `ping` | `ping` inside `button-click{id=ping}`. |
| Click `hide status items`, then `show status items` | The bar goes and the card grows to the bottom, then the bar comes back and the card shrinks. |
| `LAYOUT_TOUR=zoom WINDOW_TOUR=status-items cargo run dev`, zoom `b` | `b` fills the body, between the title bar and the status bar. |
| `{"theme": {"mode": "light"}}`, and `{"theme": {"ui_density": "compact"}}` | The bar follows the theme, and its padding the density. |

### Feature 3: modal-layer

`WINDOW_TOUR=modal-layer` places a card as the root with two buttons, `open modal` and `open dimmed modal`. Each opens a modal drawn with the UI kit's `Modal`: the headline `window_tour`, a line of text, a close button, and a footer button `Done`. Its close button and `Done` each log `modal closed`.

| Command and action | What to look for |
|---|---|
| `WINDOW_TOUR=modal-layer cargo run dev`, click `open modal` | The modal, centred near the top of the window, over the card; the window is not dimmed. `modal-show{modal=…}` in the log. |
| Press escape | It closes. `modal-hide{…, reason=escape}`, then `focus restored`. |
| Open it, click outside it | It closes, `reason=click-outside`, and the button under the click is not pressed. |
| Open it, click `Done`, then open it and click its close button | It closes each time, `reason=dismiss-event`, with `modal closed` from the extension. |
| Click `open dimmed modal` | The window behind is dimmed, as Zed's faded background. |
| `{"theme": {"mode": "light"}}` | The modal and the dimmed background follow the theme. |

### Feature 4: toasts

`WINDOW_TOUR=toasts` places a card as the root with three buttons, `info`, `warning`, and `error`, which show a toast of each kind with a line of text. The error toast has a button `Retry`, which logs `retry clicked`.

| Command and action | What to look for |
|---|---|
| `WINDOW_TOUR=toasts cargo run dev`, click `info` | The toast rises near the bottom of the window, centred, with the muted `Info` icon. `toast-show{kind=info}` in the log. |
| Wait 10 seconds | It goes away. `toast-hide{kind=info, reason=timeout}`. |
| Click `warning`, keep the pointer over the toast for 15 seconds, then move it away | It stays while the pointer is over it, and goes away a few seconds after. `timer paused`, `timer resumed`. |
| Click `error`, then `Retry` | `toast-action{kind=error, label=Retry}` with `retry clicked` inside, then `reason=action`. |
| Click `info`, then `warning` | The warning replaces the information, `reason=replaced`. |
| Click a toast's close button | It closes, `reason=dismiss-button`. |
| `{"theme": {"mode": "light"}}` | The toast follows the theme. |
