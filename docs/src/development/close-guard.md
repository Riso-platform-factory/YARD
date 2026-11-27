# close-guard: asking before a window closes or the application quits

An application made from YARD edits things: a note, a form, a document. Today a window closes as soon as the user asks. The application quits when its last window closes or on Cmd+Q / Ctrl+Q, and no extension is asked first, so unsaved work is lost.

This slice lets an extension do two things:

- **Mark a window as edited.** On macOS this shows the dot in the close button.
- **Guard a close or a quit.** The guard answers allow, cancel, or ask the user through YARD's prompt, and the answer can wait for a save.

The design follows Zed's `workspace` (`prepare_to_close`, `CloseIntent`, `prepare_windows_to_quit`) and GPUI's window-close hook. It was read in the research for this slice; the sources are in the proposal.

## What GPUI gives

- **Closing a window can be cancelled.** `Window::on_window_should_close` returns `false` to keep the window. YARD already answers every close request itself (`window-close-request`), and closes the window later with `remove_window`.
- **Quitting cannot be cancelled.** `App::on_app_quit` runs after the decision. So, as in Zed, the guard runs before YARD calls `cx.quit()`: in the `yard::Quit` action (Cmd+Q / Ctrl+Q, the app menu's Quit) and on the last-window path.
- **The edited dot.** `Window::set_window_edited` shows it on macOS and does nothing on Linux and Windows.

## What an extension writes

```rust
pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    let notes = ext.clone();
    ext.on_close_request(cx, move |intent, window, cx| {
        if !has_unsaved_notes(window, cx) {
            return CloseDecision::Allow;
        }
        CloseDecision::prompt(
            Prompt::new(PromptLevel::Warning, "Save your notes before closing?", &["Save", "Don't Save", "Cancel"]),
            move |answer, window, cx| match answer {
                Some(0) => CloseOutcome::Wait(save_notes(window, cx)), // Task<bool>
                Some(1) => CloseOutcome::Allow,
                _ => CloseOutcome::Cancel,
            },
        )
    });
}

// When a note changes, or is saved:
ext.set_window_edited(window, true, cx);
```

- **`Extension::set_window_edited(window, edited, cx)`** records the window as edited for this extension.
  - The window shows edited while any extension says so, so two extensions never overwrite each other.
  - GPUI's `set_window_edited` is called only when that changes.
  - `yard_shell::is_window_edited(window, cx)` reads the combined state.
- **`Extension::on_close_request(cx, guard)`** registers one guard per extension, as `on_open_url` does; a second one is refused with an `ERROR`.
  - The guard gets the `CloseIntent`: `CloseWindow` when one window closes, `Quit` when the application quits.
  - It returns a `CloseDecision`: `Allow`, `Cancel`, `Wait(Task<bool>)`, or `Prompt`.
  - `Prompt` shows YARD's prompt (`yard_shell/prompt`) on that window, and the extension's answer gives a `CloseOutcome`: `Allow`, `Cancel`, or `Wait`.
  - Closing the prompt without an answer counts as `Cancel`.

## How guards run

- **In the order the extensions are listed**, one after the other, on the window being closed.
  - `Allow` goes on to the next guard.
  - `Cancel` stops at once, and the window stays.
  - `Wait` waits for its task, with no time limit, as Zed does.
  - The window closes only when every guard allowed.
- **Every way to close a window goes through the guards:**
  - the system's close (the red button, the Windows caption button, WM_CLOSE, Wayland and X11 close);
  - the `yard::CloseWindow` action;
  - **the Linux drawn close button**, which today calls `remove_window` directly and skips them. It will dispatch `yard::CloseWindow`, as Zed's does.
- **Quit** (`yard::Quit`, Cmd+Q / Ctrl+Q, the app menu):
  - Guards run with `Quit` on every open window, the active window first. A window is brought to the front before its prompt.
  - If any guard on any window cancels, the quit stops and every window stays open.
  - A window closed during the pass is skipped, not counted as a cancel. A guard's `Wait` task still runs to its end first, so a save is never cut off. A prompt whose window goes away stops waiting for its answer, and the window counts as gone: a native prompt (the macOS sheet) is never answered once its window is closed.
  - A window opened during the pass gets its guards too: the pass checks the open windows again until none is left unchecked, at most 8 rounds. After that the quit is cancelled with an `info` `windows keep opening`.
  - Only when every window allowed does YARD call `cx.quit()`, and the session keeps every window as today.
- **The last window.** Closing it runs the guards once with `CloseWindow`. When they allow, the application quits as today, without asking again.
- **One at a time.** A second close request for a window whose guards are still running is ignored with an `info`, and so is a second quit. A quit while a window's close guards run is ignored with an `info` `a close is in progress`. Nothing stacks two prompts.
- **An extension turned off** in the settings (extension-toggle) registers no guard.

## What cannot be guarded (GPUI limits)

None of these runs the guards; each is a TODO with its upstream cause:

- **macOS:** the Dock's Quit, logout and shutdown, and a quit Apple Event. GPUI's application delegate has no `applicationShouldTerminate:`.
- **Windows:** logout and shutdown. GPUI answers `WM_QUERYENDSESSION` with yes.
- **Linux:** session logout and `SIGTERM`. GPUI has no session-manager hook.
- **Every system:** force quit and power loss.

Extensions keep data safe there by saving as they go: the store and node state already do, and `data-store` writes at once.

## Logging

- `close-guard-register{extension}`: `debug` registered; an `ERROR` in the diagnostic log (`log::error!`, which the lifecycle trace receives once) for a second guard of the same extension.
- `window-edited{window, extension, edited}`: `debug` when the window's combined state changes, `trace` otherwise.
- `window-close-request{window}`, already there:
  - gains `intent`;
  - a `debug` per guard (`allowed`, `cancelled`, `prompted`, `waiting`);
  - `info` `cancelled` with the extension that cancelled;
  - `info` `already in progress` for a repeated request.
- `app-quit`, already there: it now surrounds the guard pass over the windows, with `windows` and its outcome (`quitting` or `cancelled`).

## Parts

The change goes through production parts, as `extension-toggle` did: the close request lives in `yard_shell/shell`, and the drawn controls in `yard_ui`. Each changed production part is shown to the user and stamped again only after approval.

- **`yard_shell/close_guard`** (new dev part, `crates/yard_shell/src/dev/close_guard.rs`): `CloseIntent`, `CloseDecision`, `CloseOutcome`, `Extension::on_close_request`, `Extension::set_window_edited`, `is_window_edited`, and running the guards. Production code cannot use dev code, so the next item is needed.
- **`yard_shell/shell`** (production, changed): the close request and `yard::Quit` call the guard runner through a hook that `close_guard` installs. Without a guard registered, they behave exactly as today. It also handles `yard::CloseWindow` as a close request of the active window.
- **`yard_ui/window_controls`** (production, changed): it defines the action `yard::CloseWindow`, because `yard_ui` sits below `yard_shell`, which handles it. The Linux drawn close button dispatches it instead of calling `remove_window`.
- **Acceptance tests**, written first by a worker that does not write the code, with GPUI's test platform (`simulate_close`, `simulate_prompt_answer`):
  - no guard: closing and quitting are as today;
  - one guard each of `Allow`, `Cancel`, `Prompt` answered with each button, and `Wait` true and false;
  - two extensions' guards, in order, the first cancelling;
  - quit with two windows, the second cancelling: both stay;
  - the last window allowed: the application quits once, with no second pass;
  - a repeated request while a prompt is open: one prompt;
  - `set_window_edited` from two extensions: the combined state and the calls to GPUI;
  - a second guard of the same extension refused;
  - the groups, fields, and levels above, in their own files.
- **Test extension** `examples/close_tour`, named in `review.rs`. Without `CLOSE_TOUR` it does nothing. With `CLOSE_TOUR=on` it places a card with an **Edit** button (marks the window edited) and a **Save** button (clears it). Its guard asks "Save changes?" with Save, Don't Save, and Cancel; Save waits one second, then allows. The README walks through:
  - the edited dot;
  - each button, with the red button, Cmd+Q, and the menu's Quit;
  - two windows on Cmd+Q;
  - the last window;
  - `CLOSE_TOUR=cancel`, which always cancels;
  - turning the tour off in the settings;
  - the Dock's Quit, which is not guarded (shown as the documented limit).
- **Run by the lead on macOS** with `cargo run dev`. The Linux and Windows runners run the same cases from YARD-testing's `features/close-guard/`.

## Not in this slice

- Guarding the Dock's Quit, logout, and shutdown. `TODO(close-guard-session-end)`: they need `applicationShouldTerminate:` on macOS, `WM_QUERYENDSESSION` on Windows, and a session-manager hook on Linux in GPUI.
- An edited mark in the drawn title bar on Linux and Windows (macOS's dot only).
- Quitting with no window open (roadmap #30 `app-lifecycle`).
- A "confirm quit" setting. An application can write it as a guard.
- Guards per window, as subscriptions. There is one guard per extension, and it gets the window.
