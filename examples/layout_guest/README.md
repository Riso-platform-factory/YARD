# layout_guest

A second test extension for the layout engine, the guest of `layout_tour`. It places its own card in a slot of the host's card. Removing it shows that the host's slot is then empty.

When `app/riso/src/dev/review.rs` lists it as `("layout_guest", layout_guest::init)`, after `layout_tour`, and `app/riso/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it with the name the application gives it. Feature 3 is in production, so it is not loaded now; add both back with `layout_tour` to check it again.

Without a case it does nothing. It has no stage and is never in production.

## Cases

The case is chosen with the environment variable `LAYOUT_GUEST`. An unknown case logs a WARN that names the known ones. Every case runs inside a `layout-guest` group, inside `extension-init{extension=layout_guest}`.

| Command | What to look for |
|---|---|
| `LAYOUT_TOUR=host cargo run dev` | The host's `root` card, with its `body` empty: the guest does nothing. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=card cargo run dev` | The guest's card, `guest`, saying `from layout_guest`, inside the host's `root` card. In the trace, its `node-place` sits inside `extension-init{extension=layout_guest}`, and its `component-create` shows `extension=layout_guest`, nested in the host's. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=foreign cargo run dev` | The guest places a node of `layout_tour.card`. Refused with an ERROR (`extension layout_guest cannot place component layout_tour.card`); the host's card is empty. |
| `LAYOUT_TOUR=host LAYOUT_GUEST=namespace cargo run dev` | The guest registers `layout_tour.badge`. Refused with an ERROR (`extension layout_guest cannot register component layout_tour.badge`). |
| `LAYOUT_GUEST=card cargo run dev` | No host. The guest's placement is refused with an INFO (`the parent is not in the tree`), and the window is empty. |

## Removing it

Remove it from the list of extensions in `app/riso/src/dev/review.rs` and from the dependencies and the `dev` feature in `app/riso/Cargo.toml`. The host's slot is then empty, and the app runs as it should.
