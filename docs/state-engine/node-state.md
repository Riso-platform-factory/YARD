# State feature 1: node-state

## What it adds

A component can keep a small state for its node across a restart: the card of the layout tour counts to 3, the application quits, and at the next start the card shows 3. The component gives its state as JSON when the application quits and receives it back when its node's view is first created. Each window keeps its own nodes' states, as each window keeps its own layout.

## How it works

As the roadmap's decisions say:

- **Who has state.** A component whose views keep state registers it, as feature 14 registers titles: `Extension::register_state::<V>(cx, component)`, where the component's views are `Entity<V>` and `V: NodeState`:
  - `save_state(&self, cx) -> serde_json::Value`: only what is needed to build the view again;
  - `restore_state(&mut self, state, window, cx)`: called once, when the node's view is first created, before it is drawn.
  It is refused with an `ERROR` for an invalid ID, a component outside the extension's namespace, or a second registration.
- **What is saved.** The windows are the session's (window engine feature 10), in its order, the main window first; a window whose states cannot be read keeps its place with no states. For each, every node that has a view of a component with registered state: its node ID, its component's ID, and its state; and every saved state not given yet in this run, kept as it was read, so that a node that has no view this run keeps its state for the next. Copies of item slots are not saved (`item-state`, later).
- **When.** When the application quits, before its windows close. Closing the last window quits the application, so that window's states are taken when it closes, before its views are dropped, as the session keeps it.
- **Where.** In `state.json` in the data directory, named by `riso_paths` (`Paths::state_file`), written with foundation 6's atomic write. The format: a `version` (1) and the `windows`, each a map from node ID to its `component` and `state`. A new crate without GPUI, `riso_state`, owns the format, as `riso_layout/saved` owns the layout's.
- **Given back.** At the start, the window opened `i`-th from the session gets the `i`-th window's states; with no session, the one window gets the first. When a node's view is first created and the state file holds a state for its ID:
  - with the same component, `restore_state` is called with it;
  - with another component, it is not given, with a `debug` `state skipped {node, saved, component}`;
  - `restore_state` getting a state it cannot read is the component's own `WARN`; the view keeps its default.
  A state is waiting from the window's opening until the node's view is first created, even if the node is added to the tree later in the run, and is given once: a view created again later in the run (a node removed and added again, a switch of named layout that drops and creates it) starts from its default. Removing a node drops its waiting state.
  - a node whose view is not of the type its component registered: an `ERROR` once per node, as feature 14 logs a wrong title type; nothing is saved or given for it. A window opened with `NewWindow` gets no state.
- **A missing, unreadable, or other-version file** means a start without state: `debug` for missing, `WARN` for the others, never an `ERROR`.

## What code can do

```rust
// riso_paths (dev part `state_file`)
impl Paths {
    pub fn state_file(&self) -> PathBuf;
}

// riso_shell::dev
pub trait NodeState: Render {
    fn save_state(&self, cx: &App) -> serde_json::Value;
    fn restore_state(&mut self, state: serde_json::Value, window: &mut Window, cx: &mut Context<Self>);
}
impl Extension {
    pub fn register_state<V: NodeState + 'static>(&self, cx: &mut App, component: &str);
}
pub fn set_state_file(cx: &mut App, file: PathBuf);
/// The length of the user layer of `window`, as feature 8's calls given a
/// window act on it; a view uses it to show its own window's count.
pub fn window_user_layer_len(window: &Window, cx: &App) -> usize;
```

`riso_state` (new crate, no GPUI): the format's types and `encode`/`decode`, with its own logging.

## Logging

| Situation | Log |
|---|---|
| A component registers state | A group `state-register{component}`, `debug` `registered`; an `ERROR` for a refusal |
| Reading at the start | A group `state-restore{file}`, ending with `debug` `read {windows, nodes}`, or `no saved state`, or the `WARN` |
| A state given to a view | `debug` `state given {node}` in the view's `component-create` |
| Saving | A group `state-save{windows, nodes}` holding the views' states being read and `file-write{path}` |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_state` (new crate, no GPUI) | The format; its first part is dev |
| `riso_paths/state_file` (new, dev) | `Paths::state_file`: `state.json` in the data directory |
| `riso_shell/shell` dev version | `NodeState`, `register_state`, `set_state_file`, taking states at quit and at the last close, giving them back at view creation |
| `riso/startup` | Dev copy that gives the shell its state file. While `node-state` is under review, `cargo run dev` loads `review` and `layout_tour` on it |
| `examples/layout_tour` (no stage) | The card registers its count as its state; the `user layer: N` label shows its own window's count with `window_user_layer_len` |

## Tests

- **`riso_state`, unit tests:** encode and decode give the same windows; a broken file, another `version`, and unknown fields are handled as above.
- **Paths, unit test:** `state_file()` is `state.json` in the data directory.
- **Shell, GPUI tests, with the dev shell:** a stateful card counted to 3, quit, a new run with the file: the card shows 3; a node whose component changed gets no state; two windows keep their own states in order; a window from `NewWindow` gets none; a view created again in the run starts from its default; the last window's states are kept when closing it quits; the registration refusals.
- **Logging**, alone in its file: `state-restore{file}`, `state given {node}` inside `component-create`, and `state-save{windows, nodes}` holding `file-write`.

## Test extension: `examples/layout_tour`

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-state LAYOUT_TOUR=user-layer cargo run dev`, click `count` on `a` three times, quit | `state-save{windows=1, nodes=…}`; `state.json` holds `a`'s count. |
| Start again | `a` shows `count: 3`; `state given {node=a}`. |
| Press `cmd-shift-n`, count `c` in the new window twice, quit, start | Both windows are back (feature 10), each with its own counts. |

## Under review

- `riso_state` (its first part), `riso_paths/state_file`, and `riso_shell/shell`, `riso/startup` in their dev versions

## Not in this feature

- Saving a short time after a change, and keeping state after a crash: feature 2, `save-on-change`.
- The selected tab and the zoom: feature 3, `view-state`.
- State for each copy of an item slot: `item-state`, by need.
