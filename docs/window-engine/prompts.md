# Window engine feature 5: prompts

## What it adds

An extension asks the user a question that must be answered before the window can be used again, drawn by the OS, as Zed asks "Save changes?". The question has a level, a message, an optional detail, and buttons. The extension is told which button the user chose.

## How it works

As Zed does with GPUI's `Window::prompt`:

- **Drawn by the OS.** On macOS, the question is an alert attached to the window, drawn by macOS in its own style, with the level as the alert's style: information, a warning, or critical. On macOS 26 it is a panel in the middle of the window: the message in bold, the detail below it, then the buttons; the first button is the default, drawn in the accent color. Two buttons sit side by side, the first on the right; three or more are stacked, the first on top.
- **The window waits.** While the sheet is shown, the window takes no keys and no clicks. Other windows of the application still work.
- **The keys,** as GPUI sets them on macOS: return chooses the first button; a button named `Cancel` is chosen with escape; the keyboard focus starts on the last button that is neither the first nor `Cancel`, so that space reaches it.
- **The answer** is the index of the chosen button in the order given. The extension gets it in a callback. If the question goes away without an answer, because its window went away, the callback gets none. macOS does not let the user close a window while its question is shown, so this happens only when code closes the window.
- **One at a time per window.** A second question asked in a window that already shows one waits, and appears when the first is answered.

## What code can do

```rust
// yard_shell
pub struct Prompt { .. }
impl Prompt {
    /// A question with its level, its message, and its buttons, in order.
    pub fn new(level: gpui::PromptLevel, message: impl Into<SharedString>, answers: &[&str]) -> Self;
    pub fn detail(self, detail: impl Into<SharedString>) -> Self;
}

impl Extension {
    /// Asks `prompt` in `window`. `on_answer` gets the index of the chosen
    /// button, or `None` if the question went away without an answer.
    pub fn prompt(
        &self,
        prompt: Prompt,
        window: &mut Window,
        cx: &mut App,
        on_answer: impl FnOnce(Option<usize>, &mut App) + 'static,
    );
}
```

These are the only new public items. A button's label goes to GPUI as `&str`, as Zed passes it, so GPUI makes `OK` and `Cancel` the platform's own buttons.

## Logging

| Situation | Log |
|---|---|
| An extension asks | A group `prompt{level, buttons}`, `level` one of `info`, `warning`, `critical`, `buttons` the number of buttons. It stays open until the answer comes: the waiting is instrumented with it |
| The user answers | Inside the group, `debug`: `answered` with `answer` (the index) and `label` (the button's label). The extension's callback runs inside the group, so its own lines are there |
| The question goes away without an answer | Inside the group, `info`: `no answer`. The callback gets `None` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/prompt` (new, dev) | `Prompt`, and `Extension::prompt` in an `impl Extension` of its own: it calls GPUI's `Window::prompt` and spawns the waiting for the answer, instrumented with the group |

The shell does not change, and the startup does not change.

## Tests

- GPUI tests, on GPUI's test platform:
  - asking shows one pending prompt (`has_pending_prompt`); choosing a button by its label (`simulate_prompt_answer`) gives the callback that button's index, once, for the first, a middle, and the last button;
  - the detail is optional: a question without it works the same;
  - two questions asked one after the other are answered in order, each callback getting its own answer.
- The answer `None` cannot be produced on GPUI's test platform, which never drops a pending prompt, nor in the app until code can close a window: its test comes with `new-window`.
- A logging test alone in its file: every row of the table that the test platform can produce, with exact field values, and the callback's own line inside the group.
- **Existing tests:** none changes.

## Test extension: `examples/window_tour`

A new case, `WINDOW_TOUR=prompts`. It places a card as the root with two buttons. `ask` asks a warning, `Save changes?`, with the detail `Your changes will be lost if you don't save them.` and the buttons `Save`, `Don't Save`, and `Cancel`. `ask twice` asks an information, `First question`, with the button `OK`, and right after it `Second question`, with the buttons `Yes` and `No`. Each callback logs `answered` with the label it got, or `no answer`. Use a fresh home: `YARD_HOME=/tmp/yard-window`.

| Command and action | What to look for |
|---|---|
| `WINDOW_TOUR=prompts cargo run dev`, click `ask` | An alert in the middle of the window: the message in bold, the detail, and `Save`, `Don't Save`, `Cancel` stacked, `Save` in the accent color and the keyboard focus on `Don't Save`. Clicking `ask` again does nothing while it is shown. `prompt{level=warning, buttons=3}` opens in the log. |
| Click `Don't Save` | The sheet goes. `answered{answer=1, label=Don't Save}`, then the tour's own line, inside the group, which closes. |
| Click `ask`, press return; click `ask`, press escape | `Save` is chosen, then `Cancel`. |
| Click `ask twice` | `First question`; once it is answered, `Second question` appears, with `No` and `Yes` side by side, `Yes` on the right. Two groups, each with its own answer. |

## Under review

- `yard_shell/prompt`, new

## Not in this feature

- Questions drawn by YARD instead of the OS (GPUI's `set_prompt_builder`, which Zed uses on Linux): with Linux and Windows.
- Choosing a file or a folder (`prompt_for_paths`): when an application first needs it.
