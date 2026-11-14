# Window engine feature 4: toasts

## What it adds

A layer above the window's frame that shows one short message at a time near the bottom of the window, as Zed's `ToastLayer` with its `StatusToast`. An extension shows a toast of one of three kinds, information, a warning, or an error, with a line of text and at most one button. The toast goes away by itself after 10 seconds, or when the user clicks its close button or its button.

## How it works

As Zed's `crates/workspace/src/toast_layer.rs` and `crates/notifications/src/status_toast.rs`:

- **One toast at a time.** Showing a toast while none is shown shows it. Showing one while another is shown closes the shown one and shows the new one.
- **It goes away by itself** 10 seconds after it is shown (Zed's `DEFAULT_TOAST_DURATION`). While the pointer is over the toast, the time stops. When the pointer leaves, it starts again with the time that was left, but never less than 0.8 seconds (Zed's `MINIMUM_RESUME_DURATION`).
- **Closing.** The toast closes when:
  - its time runs out;
  - the user clicks its close button;
  - the user clicks its button: the button's handler runs, and the toast closes;
  - the user clicks it with the middle button, as in Zed;
  - an extension closes it with `hide_toast`;
  - another toast replaces it.
- **The focus does not move.** The toast never takes the focus, and the focused node stays focused while it is shown and after it closes.
- **Where it is drawn.** Centred horizontally, its bottom edge 40 px above the bottom of the window, so it floats just above the status bar. It enters as in Zed: it rises from the bottom of the window to its place and fades in from 0.4 opacity, in 150 ms. It is drawn over the frame, title bar, body, and status bar, and under the modal layer: an open modal stays above a toast.
- **The mouse.** The toast blocks the mouse from what is below it. The rest of the layer blocks nothing: a click beside the toast reaches the view under it.
- **Its look**, as Zed's `StatusToast`: one row with the elevation of a modal (`elevation_3`), Zed's `surface_background`, a large shadow, a gap of `gap_2`, padding `py_1p5`, `pl_2p5`, and `pr_1p5`. In the row, in this order:
  - the kind's icon at the small size: information is `Info` in the muted color, a warning is `Warning` in the warning color, an error is `XCircle` in the error color, as Zed's `Callout` and `Banner` draw their severities;
  - the text, as a label in the default color;
  - the button, if the toast has one: Zed's `Button` with the button's label, in the muted color;
  - the close button: Zed's `IconButton` with the `Close` icon, square, small icon, muted color.
- **Without a toast,** the layer draws nothing and takes no room.
- Zed's `toast::RunAction` (`alt-shift-enter` runs the toast's button), tooltips on the buttons, and toasts without a close button or without a timer are not part of this feature.

## What code can do

```rust
// riso_ui
pub enum ToastKind { Info, Warning, Error }

/// What a toast shows.
pub struct Toast { .. }
impl Toast {
    pub fn new(kind: ToastKind, text: impl Into<SharedString>) -> Self;
    /// The toast's one button. A second call replaces the first.
    pub fn action(
        self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self;
}

pub enum ToastHideReason { Timeout, DismissButton, Action, MiddleClick, HideToast, Replaced }

/// A GPUI view, held by the shell.
pub struct ToastLayer;
impl ToastLayer {
    pub fn new() -> Self;
    pub fn show_toast(&mut self, toast: Toast, cx: &mut Context<Self>);
    pub fn hide_toast(&mut self, reason: ToastHideReason, cx: &mut Context<Self>);
    pub fn has_active_toast(&self) -> bool;
}

// riso_shell
impl Extension {
    pub fn show_toast(&self, toast: riso_ui::Toast, window: &mut Window, cx: &mut App);
    pub fn hide_toast(&self, window: &mut Window, cx: &mut App);
}
pub fn has_toast(window: &Window, cx: &App) -> bool;
```

These are the only new public items. `ToastLayer` keeps every type of Zed inside `riso_ui`; its API uses riso's and GPUI's types only. Each window's shell holds its own layer.

For tests, the toast's elements have the debug selectors `TOAST` (the row), `TOAST-ACTION` (its button), and `TOAST-DISMISS` (its close button).

## Logging

| Situation | Log |
|---|---|
| A toast is shown | A group `toast-show{kind}`, `kind` one of `info`, `warning`, `error`. Inside it, `debug`: `timer started` with `duration_ms=10000` |
| A toast is closed | A group `toast-hide{kind, reason}`, `reason` one of `timeout`, `dismiss-button`, `action`, `middle-click`, `hide-toast`, `replaced` |
| A toast replaces another | `toast-hide` of the old one with `reason=replaced`, then `toast-show` of the new one |
| The user clicks the toast's button | A group `toast-action{kind, label}` around the extension's handler, so its own lines are inside, then `toast-hide` with `reason=action` |
| The pointer enters or leaves the toast | `debug`: `timer paused` with `remaining_ms`, and `timer resumed` with `remaining_ms` |
| `hide_toast` with no toast shown | Nothing |
| The layer draws a toast | One `trace` line per draw, with the toast's kind. Nothing while no toast is shown |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_ui/toast_layer` (new, dev) | `ToastKind`, `Toast`, `ToastHideReason`, and `ToastLayer`, adapted from Zed's `toast_layer.rs` and `status_toast.rs`, with Zed's notice: the state, the timer, the closing, and the drawing. One change from Zed: the time left is measured with GPUI's clock, `BackgroundExecutor::now`, instead of `Instant::now`, so that the test clock drives it |
| `riso_shell/shell` | Dev version beside production. Each shell holds a `ToastLayer` and draws it above the frame and below the modal layer; `show_toast`, `hide_toast`, `has_toast` |

The startup does not change.

## Tests

- GPUI tests, in a window of a known size, with a root card that has a button:
  - showing a toast draws it centred horizontally, its bottom edge 40 px above the window's bottom once its entry has ended; `has_toast` is true; the focused node is still focused;
  - each kind draws its icon and the text; a toast with a button draws the button between the text and the close button; one without draws no `TOAST-ACTION`;
  - after 10 seconds of the test clock, it is gone and `has_toast` is false; just before, it is still there;
  - with the pointer over it, the clock passes 10 seconds and it stays; when the pointer leaves, it closes when the time that was left has passed; when less than 0.8 seconds was left, it closes after 0.8 seconds;
  - its close button closes it; its button runs the handler once and closes it; a middle click closes it;
  - a click on the toast does not reach the card's button under it; a click beside it reaches the card's button;
  - `hide_toast` closes it; `hide_toast` with no toast shown changes nothing;
  - showing a second toast replaces the first; the first one's timer then closes nothing;
  - with no toast shown, the layer takes no room and blocks nothing;
  - with a toast shown and a modal open, a click where the toast is closes the modal and does not close the toast.
- A logging test alone in its file: every row of the table, with exact field values, and the draw line counted over every crate's lines.
- **Existing tests:** none changes. Any failing production test is a defect.

## Test extension: `examples/window_tour`

A new case, `WINDOW_TOUR=toasts`. It places a card as the root with three buttons, `info`, `warning`, and `error`, which show a toast of each kind with a line of text. The error toast has a button `Retry`, whose handler logs `retry clicked` from the extension. Use a fresh home: `RISO_HOME=/tmp/riso-window`.

| Command and action | What to look for |
|---|---|
| `WINDOW_TOUR=toasts cargo run dev`, click `info` | The toast rises near the bottom of the window, centred, above the status bar, with the muted `Info` icon. `toast-show{kind=info}` in the log. |
| Wait 10 seconds | It goes away. `toast-hide{kind=info, reason=timeout}`. |
| Click `warning`, keep the pointer over the toast for 15 seconds, then move it away | It stays while the pointer is over it, and goes away a few seconds after. `timer paused`, `timer resumed`. |
| Click `error`, then `Retry` | `toast-action{kind=error, label=Retry}` with `retry clicked` inside, then `reason=action`. |
| Click `info`, then `warning` | The warning replaces the information, `reason=replaced`. |
| Click a toast's close button | It closes, `reason=dismiss-button`. |
| `{"theme": {"mode": "light"}}` | The toast follows the theme. |

## Under review

- `riso_ui/toast_layer`, new
- `riso_shell/shell`, in its dev version

## Not in this feature

- Zed's `toast::RunAction` and its key `alt-shift-enter`: when the command palette lists actions.
- Tooltips on the toast's buttons: with the UI kit's `tooltip`.
- Zed's stack of notifications in the bottom right corner (`notifications.rs`), and toasts that stay until closed: when an application first needs them.
- Toasts in several windows at once: they work per window already; their test comes with `new-window`.
