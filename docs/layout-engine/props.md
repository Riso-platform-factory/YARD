# Layout feature 13: props

## What it adds

A node carries props: a small configuration given when it is placed, such as the title of a card or the file a panel shows. A component that takes props declares their type, with a default for every value, and its view is created from them. Props that cannot be read are logged, and their defaults are used. While the application runs, a new operation, `set-props`, changes some of a node's props: the view is told, and the operation is recorded in the user layer and saved like the others.

This is how Zed's items keep what they show apart from where they are: a pane holds an item, and the item holds its own state. Here the layout keeps that small state, so that it is saved with the layout and restored with it.

## The model

In `riso_layout`, without GPUI:

- **`Props`** is a JSON object, `serde_json::Map<String, Value>`. The model does not know what the values mean; only the component does.
- **A `Node` has `props`.** A node placed without props has an empty object.
- **`set-props(node, props)`** is a new operation. It sets each key it names to the value given, and leaves the other keys as they were. Setting a key to `null` removes it, so that the default is used again.
- **Model rule:** the node must be in the tree, or the operation is refused with an `INFO`, as for `hide`. The root can have its props set.
- Props stay with a node when it is moved, hidden, or shown, and go with it when it is removed.
- **The user layer** records `set-props` like the other operations. A reset drops it with them, so every node has its default props again.

## Components with props

In the shell:

- **A component declares its props type** when it is registered, with `register_with_props::<P, V>`. `P` is a Rust type that is `Serialize`, `Deserialize`, and `Default`, and `V` is the component's view. The view implements `PropsView<P>`, whose one method, `set_props`, is how it is told of new props.
- **Reading props.** The shell reads a node's props into `P` key by key, starting from `P::default()`:
  - each key whose value fits is used;
  - a key whose value does not fit `P` keeps the default, with a `WARN`: `prop {key} of node {node} cannot be read: {error}; the default is used`;
  - a key that `P` does not have is ignored, with a `WARN`: `node {node} has an unknown prop {key}`.

  So one bad value never costs the others. Reading happens when the view is created and each time the props change.
- **Placing with props.** `place_with_props`, `place_root_with_props`, and `add_with_props` are `place`, `place_root`, and `add` with a props value. The value is any `Serialize` that encodes as a JSON object. One that does not is an `ERROR`, a bug of the extension, and the node is not placed. `place`, `place_root`, and `add` place a node with no props.
- **Changing props.** `Extension::set_props(cx, node, props)` runs `set-props` through the same path as the other operations, with the shell's rules: the props must encode as a JSON object (otherwise an `ERROR`, and nothing changes). Any extension may set the props of any node, as any extension may move one.
- **Telling the view.** After an applied `set-props`, the shell reads the node's props into `P` again and calls the view's `set_props`. A node whose component was registered without props keeps its props in the model, and its view is not told.
- **After a reset,** each node whose props are now different from what its view was last given is told its props again.
- **Restoring at startup.** A saved `set-props` follows the rules of feature 12: it is applied if its node is in the tree, and skipped with an `INFO` otherwise.

## What code can do

```rust
// riso_layout (production, in place)
pub type Props = serde_json::Map<String, serde_json::Value>;
pub struct Node { pub id: NodeId, pub component: ComponentId, pub extension: String, pub props: Props }
pub enum Operation { /* … */ SetProps { node: NodeId, props: Props } }
pub enum Applied { /* … */ PropsSet }

// riso_shell::dev
pub trait PropsView<P>: gpui::Render {
    fn set_props(&mut self, props: P, cx: &mut gpui::Context<Self>);
}
impl Extension {
    pub fn register_with_props<P, V>(
        &self,
        cx: &mut App,
        component: &str,
        slots: &[&str],
        create: impl Fn(P, &NodeId, Slots, &mut Window, &mut gpui::Context<V>) -> V + 'static,
    ) where
        P: serde::Serialize + serde::de::DeserializeOwned + Default + 'static,
        V: PropsView<P> + 'static;
    pub fn place_root_with_props(&self, cx: &mut App, node: &str, component: &str, props: impl serde::Serialize);
    pub fn place_with_props(&self, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, props: impl serde::Serialize);
    pub fn add_with_props(&self, window: &mut Window, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str, props: impl serde::Serialize);
    pub fn set_props(&self, cx: &mut App, node: &str, props: impl serde::Serialize);
}
```

The dev shell has every other item of the production shell, unchanged.

## What is saved

`layout.json` keeps `version` 1. The format grows in a way that old files can still be read:

- an `add` has a `props` object when the node was added with props, and none otherwise;
- a new kind, `props`, with `node` and `props`.

Decoding checks that `props` is a JSON object. A file whose `props` is not an object cannot be read, as the rules of feature 12 say: a `WARN`, and the default layout.

## Logging

| Situation | Log |
|---|---|
| A `set-props` runs | The group `layout-op{op=props, node, extension}`, as other operations. Inside it, the model logs `props set {keys}` at `debug`, or the refusal at `INFO`; then `props-read{node, component}` |
| A node's props are read into its type | A group `props-read{node, component}`, with `debug` `props read {keys}`, and a `WARN` for each key that cannot be read or is unknown |
| Props that do not encode as a JSON object | `ERROR`, in the group of the call (`node-place`, `root-place`, or `layout-op`) |
| A view is told new props | `debug` `view told {node}`, in the group of the operation or of the reset |

## Where the code changes

| Part | What changes |
|---|---|
| `riso_layout/tree` (production, in place) | `Props`, `Node::props`, `Operation::SetProps` with its rule, and `Applied::PropsSet`. Stamped again. |
| `riso_layout/saved` (production, in place) | The `props` of `add` and the kind `props`. Stamped again. |
| `riso_shell/shell` (production, in place) | Only what the new field and variant make necessary for it to compile and to keep its behavior: a node it places has empty props, and it treats `set-props` as unknown to it. Stamped again. |
| `riso_shell/shell` dev version | `register_with_props`, `PropsView`, the `*_with_props` placements, `set_props`, reading props, telling views, and the reset. |
| `riso/startup` | Dev copy that uses the dev shell. While `props` is under review, `cargo run dev` loads `review` and `layout_tour` on it, and not the command palette, which is built on the production shell's extension handle |

## Tests

- **Model:**
  - `set-props` merges keys, and `null` removes one;
  - it is refused for a node not in the tree, and the tree is unchanged;
  - props stay through `move` and `hide`, and go with `remove`;
  - the user layer records it, and a reset gives the default props back.
- **Saved format:**
  - an `add` with props and a `props` operation encode and decode back to the same operations;
  - a file of feature 12, with no props anywhere, still decodes;
  - a `props` that is not an object is refused.
- **Shell, GPUI tests, with the dev shell:**
  - a component with props creates its view from the node's props;
  - a missing key gives its default;
  - a bad value keeps its default and does not cost the other keys, with a `WARN`;
  - an unknown key is ignored with a `WARN`;
  - `set_props` tells the view the merged props;
  - a reset tells it the default props;
  - props that do not encode as an object are an `ERROR`, and nothing is placed or changed;
  - a saved `set-props` is restored at the next start.
- **Logging**, alone in its file: `layout-op{op=props}` holds `props set` and a `props-read` group; a bad value logs one `WARN`.

## Test extension: `examples/layout_tour`

A new case, `props`. Its card `layout_tour.tinted` takes props `title` (text, default `Untitled`) and `tint` (one of `default`, `accent`, `success`, `warning`, `error`; default `default`). It shows its title in a headline, colored by its tint through the palette. The default layout is the controls, then a row with `alpha` (`title: Alpha`, `tint: accent`), `beta` (no props), and `broken` (`title: Broken`, `tint: 42`).

| Command and action | What to look for |
|---|---|
| `RISO_HOME=/tmp/riso-props LAYOUT_TOUR=props cargo run dev` | `Alpha` in the accent color, `Untitled` in the default color, and `Broken` in the default color. One `WARN`, `prop tint of node broken cannot be read`. |
| Click `rename beta` | `beta` shows `Beta`. `layout-op{op=props, node=beta}` holds `props set {keys=title}`, `props-read`, and `view told {node=beta}`. |
| Click `tint beta warning` | `Beta` in the warning color; its title is kept. |
| Click `clear alpha's tint` | `Alpha` in the default color. |
| Click `props of ghost` | Refused with an `INFO`: the node is not in the tree. |
| Quit and start again with the same command | `layout-restore` ends with `restored {applied=3, skipped=0}`: `Beta` in the warning color, `Alpha` in the default color. `layout.json` holds three `props` operations. |
| `reset layout` | `Alpha` in the accent color again, and `Untitled`. `view told` for `alpha` and `beta`. |

## Under review

- `riso_layout/tree`, `riso_layout/saved`, and production `riso_shell/shell`, in place
- `riso_shell/shell` and `riso/startup`, in their dev versions

## Not in this feature

- A node's title and icon in a tab: feature 14, `titles`, which reads them from props.
- Props that one component gives to the components below it: feature 18, `contexts`.
- Undo of a props change.
