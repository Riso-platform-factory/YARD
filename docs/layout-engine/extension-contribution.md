# Layout feature 3: extension-contribution

## What it adds

Every registration and every placement belongs to one extension. An extension places its own nodes, and it may place them in a slot of a node that another extension placed. Remove that extension, and the slot is empty.

## Model

- An **extension** has a name, which is a slug, such as `layout_tour`.
- The application gives each extension its name. It does so in the list of extensions passed to the startup, as `(name, init)` pairs. An extension does not choose its own name.
- The startup runs each `init` with a handle for that extension. Every call to the shell goes through the handle: register a component, place the root, place a node. So the shell always knows which extension did what.
- A component belongs to the extension that registered it.
- A node belongs to the extension that placed it.

## Rules

These are checked when the call is made.

| Situation | What happens | Log |
|---|---|---|
| An extension registers a component whose ID does not start with its own name, such as `layout_guest` registering `layout_tour.badge` | Refused | `ERROR`: a bug of the extension |
| An extension places a node of a component that is not in its own namespace, such as `layout_guest` placing `layout_tour.card` | Refused | `ERROR` |
| Two extensions in the list have the same name, or a name breaks the slug rule | The second one, or the bad one, is not run | `ERROR`, in `extensions-load` |

An extension may place its nodes:
- as the root;
- in a slot of its own nodes;
- in a slot of another extension's node.

The rules of feature 2 do not change:
- node IDs are unique in the whole tree;
- the parent must already be placed;
- the slot is checked against the parent's component when the window opens.

## Load order

The startup runs the extensions in the order of the list. An extension that places nodes in another one's slots comes after it in the list. If it comes before, its placement is refused because the parent is missing, as in feature 2.

## Logging

- `extensions-load` holds one group per extension, `extension-init{extension}`, around its `init`.
- Inside it, `component-register`, `root-place`, and `node-place` work as before.
- `component-create` gains the field `extension`: the extension that placed the node.

## Where the code changes

- **`riso_shell/shell`.** `register`, `place_root`, and `place` become methods of the extension's handle, `Extension`. They are no longer free functions.
- **`riso/startup`.**
  - The startup receives `(name, init)` pairs, and checks the names.
  - It creates each extension's handle, and runs each `init` with it inside `extension-init`.
  - The init function's type becomes `fn(&mut App, &Extension)`.
- **`app/riso/src/main.rs`.** Its empty list keeps working; only its type changes. It is a root file, with no code of its own.
- **`review.rs`.** Its `announce` becomes an extension named `review`.

Both `riso_shell/shell` and `riso/startup` are changed through dev versions, as in features 1 and 2. `cargo run` stays as approved until promotion.

## Test extensions

Two extensions, so that one can be removed:

- `examples/layout_tour`, named `layout_tour`, is the host. Its cases from features 1 and 2 keep working through the handle.
- `examples/layout_guest`, a new crate, named `layout_guest`, is the guest. It is listed after the host. It does nothing without its own parameter, `LAYOUT_GUEST`.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=host cargo run dev` | The host's `root` card, with an empty `body`. The guest does nothing. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=card cargo run dev` | The guest's card, `guest`, appears inside the host's `root` card. In the trace, `node-place` sits inside `extension-init{extension=layout_guest}`, and the guest card's `component-create` shows `extension=layout_guest`. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=foreign cargo run dev` | The guest tries to place a node of `layout_tour.card`. It is refused with an `ERROR`, and the host's card is empty. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=namespace cargo run dev` | The guest tries to register `layout_tour.badge`. It is refused with an `ERROR`. |
| `LAYOUT_GUEST=card cargo run dev` | No host, so the guest's parent is missing. The placement is refused with an `INFO`, and the window is empty. |

## Under review

`riso_shell/shell` and `riso/startup`, in their dev versions.

## Not in this feature

- An extension that depends on another being loaded first, beyond the order of the list.
- Layers that the user can change: feature 11.
- Props.
