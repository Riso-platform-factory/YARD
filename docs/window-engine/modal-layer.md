# Window engine feature 3: modal-layer

## What it adds

A layer above the window's frame that shows one modal at a time, as Zed's `ModalLayer`. An extension opens a modal of its own; the modal takes the focus; escape, a click outside it, or the modal itself closes it; and the focus goes back to where it was. The modal's content is the extension's own view, usually drawn with the UI kit's `Modal`.

## How it works

As Zed's `crates/workspace/src/modal_layer.rs`:

- **One modal at a time.** Opening a modal while none is open shows it. Opening one while another of the same type is open closes the open one and shows nothing: the call toggles it. Opening one while another of a different type is open closes the open one and shows the new one.
- **The modal takes the focus** once it is shown, through its own focus handle. When it closes, the focus goes back to what had it before the modal opened, if the focus is still inside the layer. When one modal replaces another, the focus goes back, at the end, to what had it before the first one.
- **Closing.** The modal closes when:
  - it emits GPUI's `DismissEvent`, which is how its own close button or buttons close it;
  - the user presses escape while the focus is inside it: the shell's new action `Dismiss`, bound to `escape` in the layer's key context `ModalLayer`;
  - the user clicks outside it, anywhere in the window;
  - an extension closes it with `hide_modal`.
- **Where it is drawn.** Over the whole window, title bar and status bar included, as Zed's layer covers its workspace: the modal is centred horizontally, 5 rems from the top of the window (Zed's `top_20`). The layer blocks the mouse from what is below it while a modal is open: a click outside the modal closes it and reaches nothing else.
- **The background.** As in Zed, the window behind is not dimmed unless the modal asks for it. A modal that asks gets Zed's faded background: the theme's elevated surface background, faded by 0.2, over the whole window.
- **Without a modal,** the layer draws nothing and takes no room, and escape does nothing of the layer's.
- **The layout while a modal is open.** The modal holds the focus, so no node is focused: `focused_node` gives none. When the modal closes, the node that was focused is focused again.
- Zed's stashed, reopenable pickers and its `on_before_dismiss` veto are not part of this feature.

## What code can do

```rust
// yard_ui
pub trait ModalView: gpui::ManagedView {      // Render + Focusable + EventEmitter<DismissEvent>
    fn dims_background(&self) -> bool { false }
}
pub struct ModalLayer;                         // a GPUI view, held by the shell

// yard_shell
impl Extension {
    pub fn toggle_modal<V: yard_ui::ModalView>(
        &self, window: &mut Window, cx: &mut App,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    );
    pub fn hide_modal(&self, window: &mut Window, cx: &mut App);
}
pub fn has_modal(window: &Window, cx: &App) -> bool;
// the action yard::Dismiss, bound to escape in the key context ModalLayer
```

`ModalLayer` keeps every type of Zed inside `yard_ui`; its API uses GPUI's types only. Each window's shell holds its own layer.

## Logging

| Situation | Log |
|---|---|
| A modal is shown | A group `modal-show{modal}`, `modal` the short name of its type, around the building of the view, so the extension's own lines are inside |
| A modal is closed | A group `modal-hide{modal, reason}`, `reason` one of `dismiss-event`, `escape`, `click-outside`, `hide-modal`, `toggle`, `replaced`. Inside it, `debug`: `focus restored` when the focus goes back |
| A toggle closes the open modal of the same type | `modal-hide` with `reason=toggle`, and no `modal-show` |
| `hide_modal` or escape with no modal open | Nothing |
| The layer draws a modal | One `trace` line per draw: the modal's name and whether the background is dimmed. Nothing while no modal is open |
| The default keys are bound | `keys-bind` counts one more key, `escape` for `yard::Dismiss`, logged as its other keys are, with the action's full name |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_ui/modal_layer` (new, dev) | `ModalView` and `ModalLayer`, adapted from Zed, with Zed's notice: the state, the focus, the closing, and the drawing |
| `yard_shell/shell` | Dev version beside production. Each shell holds a `ModalLayer` and draws it above the frame; the action `Dismiss` and its default key; `toggle_modal`, `hide_modal`, `has_modal` |

The startup does not change.

## Tests

- GPUI tests, in a window of a known size, with a test modal that has a focusable element and a close button emitting `DismissEvent`:
  - opening a modal shows it centred horizontally, 5 rems from the window's top, and focuses it; `has_modal` is true and `focused_node` gives none;
  - its close button closes it, and the node focused before is focused again;
  - escape closes it; a click outside it closes it, and does not reach a button under the click; a click inside it does not close it;
  - `hide_modal` closes it; `hide_modal` and escape with no modal open change nothing;
  - opening the same type again closes it; opening another type replaces it, and closing that one gives the focus back to the node focused before the first;
  - a modal that dims the background draws the faded background over the whole window; one that does not, draws none;
  - after one modal replaces another, a `DismissEvent` from the replaced modal's view closes nothing;
  - with no modal open, the layer takes no room and blocks nothing: a click on a card's button reaches it;
  - the modal is drawn over the title bar and the status bar: a click on a bar item while a modal is open closes the modal and does not reach the item.
  - a click outside the modal reaches no mouse-down handler of the views below it either, only the layer's.
- A logging test alone in its file: every row of the table, with exact field values, and the draw line counted over every crate's lines.
- **Existing tests:** only a test that counts or lists the default keys changes: it now has `escape` for `Dismiss`. Each change is listed when it is made.

## Test extension: `examples/window_tour`

A new case, `WINDOW_TOUR=modal-layer`. It places a card as the root with two buttons, `open modal` and `open dimmed modal`. Each opens a modal drawn with the UI kit's `Modal`: the headline `window_tour`, a line of text, a close button, and a footer button `Done` that also closes it. Its close button and `Done` each log `modal closed` from the extension; escape and a click outside are the layer's, and log only the layer's lines. Use a fresh home: `YARD_HOME=/tmp/yard-window`.

| Command and action | What to look for |
|---|---|
| `WINDOW_TOUR=modal-layer cargo run dev`, click `open modal` | The modal, centred near the top of the window, over the card; the window is not dimmed. `modal-show{modal=…}` in the log. |
| Press escape | It closes. `modal-hide{…, reason=escape}`, then `focus restored`. |
| Open it, click outside it | It closes, `reason=click-outside`, and the button under the click is not pressed. |
| Open it, click `Done`, then open it and click its close button | It closes each time, `reason=dismiss-event`, with `modal closed` from the extension. |
| Click `open dimmed modal` | The window behind is dimmed, as Zed's faded background. |
| `{"theme": {"mode": "light"}}` | The modal and the dimmed background follow the theme. |

## Under review

- `yard_ui/modal_layer`, new
- `yard_shell/shell`, in its dev version

## Not in this feature

- Zed's stashed, reopenable pickers and its `on_before_dismiss` veto: when a modal first needs them.
- Toasts and notifications: `toasts`.
- Modals in several windows at once: they work per window already; their test comes with `new-window`.
