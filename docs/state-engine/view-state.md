# State feature 3: view-state

## What it adds

The shell's own containers keep their state across a restart, as a component's view does since feature 1: the tab a tabs container shows, and the node that fills the window. Select the tab `d` in the tour, zoom it, quit, start: `d` is selected and fills the window.

## How it works

- **Who has state.** The shell registers state for two of its own components, as an extension does with `register_state`, when it starts:
  - `riso.tabs`: `{"selected": "<node ID>"}`, the tab it shows, or `null` when it shows none;
  - `riso.window`: `{"zoomed": "<node ID>"}`, the node that fills the window's body, or `null`.
  They are kept in `state.json` like any node's state, under the node's ID (the tabs container's ID, and `window`), with feature 1's rules: each window its own, given once when the view is first created, not given to another component, dropped with the node.
- **When it is saved.** Selecting a tab (a click, an action of the keymap engine, a drop, or a new item's tab), and zooming in or out once a window is open, call `state_changed`, but selecting the tab already selected does not, so that feature 2 saves them after its wait, and quitting saves them as every state. A selection the shell moves by itself, because the selected tab was hidden, removed, or became empty, does not call it: the next save takes what is shown then. The zoom out the shell does by itself, when the zoomed node becomes empty, calls it as a zoom out does: it is rare, and keeps the file close to the screen.
- **Given back, tabs.** The saved tab is selected when the window's views are created, or at the container's first layout if the container is created later, if it has a tab then (shown and not empty), even while a zoom keeps the container from being drawn. Otherwise the first tab is selected, as without a saved state, with a `debug` `saved tab has no tab {tabs, tab}`.
- **Given back, zoom.** After the window's views are created, the saved node is zoomed if it is shown, in the body, and has a view, as feature 10 of the layout engine allows a zoom. Otherwise nothing is zoomed, with a `debug` `saved zoom not given {node}`. The saved state, `null` included, replaces the zoom the extension set when it started: it is what the user last saw.
- **A named layout switch** clears the zoom, as today. A tabs container the new layout keeps keeps its view and its selection; one it creates again starts from its default, as feature 1 says for a view created again.

## What code can do

Nothing new: extensions see no new function. `selected_tab` and `zoomed` give the restored values.

## Logging

| Situation | Log |
|---|---|
| The shell registers its states | `state-register{component=riso.tabs}` and `state-register{component=riso.window}` of feature 1 |
| A tab given back | `state given {node}` of feature 1 in `component-create`; then, when the saved tab is selected, `debug` `tab given {tabs, tab}` once |
| A zoom given back | `debug` `zoom given {node}` in `shell-create` |
| A saved value that cannot be used | The `debug` lines above; a state that cannot be read (not an object, or a value that is not a string or `null`) is a `WARN`, and the default is kept |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_shell/shell` dev version | The two registrations, saving and giving back the selected tab and the zoom, `state_changed` on a selection and a zoom |
| `riso/startup` | Dev copy that uses the dev shell. While `view-state` is under review, `cargo run dev` loads `review` and `layout_tour` on it |

## Tests

- **Shell, GPUI tests, with the dev shell:**
  - tab `d` selected, quit, a new run with the file: `d` is selected;
  - a node zoomed, quit, a new run: it is zoomed; zoomed out, quit, a new run: nothing is zoomed, even when the extension zooms a node when it starts;
  - a saved tab that is hidden or empty at the next start: the first tab is selected, with the `debug`;
  - a saved zoom of a node that is hidden or outside the body: nothing is zoomed, with the `debug`;
  - two windows keep their own selections and zooms;
  - selecting a tab by a click and by an action, and zooming, each lead to one save after feature 2's wait; a selection the shell moves by itself does not;
  - a state that cannot be read: `WARN`, the default kept.
- **Logging**, alone in its file: `state-register` for `riso.tabs` and `riso.window`; `tab given {tabs, tab}` once at `debug`; `zoom given {node}` inside `shell-create`; the `WARN` for a state that cannot be read.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-view LAYOUT_TOUR=user-layer cargo run dev`, select the tab `d`, click `zoom` on `d`, quit | `state.json` holds `right`: `{"selected": "d"}` and `window`: `{"zoomed": "d"}`. |
| Start again | `d` fills the window; `zoom given {node=d}`. Click `zoom` again: `d` is the selected tab of its container. |
| Zoom out, select `c`, end the process with `kill -9`, start | `c` is selected, nothing is zoomed. |

## Under review

- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- The tab bar's scroll position: it follows the selected tab.
- State for each copy of an item slot: `item-state`, by need.
