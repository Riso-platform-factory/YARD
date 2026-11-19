# State feature 2: save-on-change

## What it adds

A component says when its state changed, and the states are saved a short time later, so that a burst of changes writes the file once and a crash loses little. Ten fast clicks on the tour's card write `state.json` once; ending the process without quitting still keeps the count.

## How it works

- **Saying it changed.** A view whose component registered state (feature 1) calls `yard_shell::state_changed(cx)` after its state changes, as the card does after a click.
- **When it is saved.** The save waits until no change has come for 300 ms, and never longer than 2 s after the first change of a burst, so that a steady stream of changes still saves every 2 s. It saves what feature 1 saves: every window's states in the session's order, with the states still waiting. A change while a save is being written queues one more save after it.
- **Windows opening and closing** count as changes too: the session (window engine feature 10) writes its windows when one opens or closes, and the state file follows it with the same wait, so that after a crash the `i`-th window of both files is still the same window.
- **Quitting** writes once more, as feature 1 does, and cancels a save that is waiting, so that the file is written once at quit.
- **Crashes.** A process that ends without quitting keeps the last state written: at most the last 2 s of changes are lost.
- **Off the main thread.** The file is written on the background executor, inside its group, as the session and the window bounds are.

## What code can do

```rust
// yard_shell::dev
/// The calling view's state changed: save the states soon.
pub fn state_changed(cx: &mut App);
```

## Logging

| Situation | Log |
|---|---|
| `state_changed` | `trace` `state changed`: it can run on every keystroke |
| The save after the wait | The group `state-save{windows, nodes}` of feature 1, with the field `reason=change` (`reason=quit` at quit) |
| A change while a save is being written | `debug` `state save queued` |

## Where the code changes

| Part | What changes |
|---|---|
| `yard_shell/shell` dev version | `state_changed`, the wait, and the save after it |
| `yard/startup` | Dev copy that uses the dev shell. While `save-on-change` is under review, `cargo run dev` loads `review` and `layout_tour` on it |
| `examples/layout_tour` (no stage) | The card calls `state_changed` after a click |

## Tests

- **Shell, GPUI tests, with the dev shell:**
  - ten changes 10 ms apart write the file once, 300 ms after the last;
  - changes every 100 ms for 5 s write the file about every 2 s, never more often;
  - the file written after the wait holds the latest states, and a new run from it gives them back (feature 1) without a quit;
  - a change while a save is being written leads to one more save with the newer state;
  - quitting with a save waiting writes the file once;
  - closing one of two windows leads to a save whose windows match the session's.
- **Logging**, alone in its file: `state changed` at `trace`; `state-save{reason=change}` after the wait and `state-save{reason=quit}` at quit, each holding `file-write`.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-soc LAYOUT_TOUR=user-layer cargo run dev`, click `count` on `a` ten times quickly | One `state-save{reason=change}` after the clicks stop. |
| End the process with `kill -9` | `state.json` holds the count. |
| Start again | `a` shows the count. |

## Under review

- `yard_shell/shell` and `yard/startup`, in their dev versions

## Not in this feature

- The shell's own containers' state (selected tab, zoom): feature 3, `view-state`.
