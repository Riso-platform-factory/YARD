# layout_inspector

A test extension for the layout engine that shows the layout tree beside any case of `layout_tour`: each node, indented by its depth, with its component, its props, its extension, and what put it there (`default`, the shown named layout's name, `user`, or `copy <key>`). A hidden node's row is drawn disabled. It follows the tree through `yard_shell::observe_layout` and reads it with `yard_shell::inspect`, so a drag, a hide, a reset, or a switch of named layout shows at once.

When `app/yard/src/dev/review.rs` lists it as `("layout_inspector", layout_inspector::init)`, after `layout_tour`, and `app/yard/Cargo.toml` depends on it as an optional dependency turned on by `dev`, `cargo run dev` loads it with the name the application gives it.

Without `LAYOUT_INSPECTOR` it does nothing. With it, `layout_tour` places a row `tour_frame` as the root, with the case's root inside it, and the inspector places its node `inspector` last in that row, 360 px wide. It has no stage and is never in production.

| Command and action | What to look for |
|---|---|
| `YARD_HOME=/tmp/yard-inspect LAYOUT_INSPECTOR=1 LAYOUT_TOUR=user-layer cargo run dev` | The case on the left, the inspector on the right: `Layout`, then `tour_frame`, `root` and the case's nodes under it, then `inspector`, indented; each `default`. |
| Drag tab `b` onto the top edge of `right`'s content | Rows `split-1` and `tabs-1` appear, `user`; `b` is under `tabs-1`. |
| Click `hide d` | `d`'s row is disabled. |
| Click `reset layout` | The rows are as at the start. |
| `LAYOUT_INSPECTOR=1 LAYOUT_TOUR=item-slots cargo run dev`, click `add dee` | A row `name@dee`, `copy dee`. |

The cases of `layout_tour` that test the root itself (`missing`, `twice`) mean something else under `LAYOUT_INSPECTOR`, because their root is placed in `tour_frame`; run them without it.
