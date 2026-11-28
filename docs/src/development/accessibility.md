# accessibility: roles and labels for screen readers

A screen reader (VoiceOver on macOS, Narrator or NVDA on Windows, Orca on Linux) reads an application through the system's accessibility tree: each control's role, name, state, and the actions it accepts. An application built on YARD should be usable with one from the start, and its extensions should be able to label their own controls.

GPUI already does the hard part at Zed rev `ddb90e5`:
- It builds an AccessKit tree from the elements that have an id and a role, and hands it to the system: `accesskit_macos`, `accesskit_windows`, and `accesskit_unix` (AT-SPI).
- It is on by default. It builds nothing until an assistive technology asks, so it costs nothing when none is running.
- All six AccessKit crates are already in `Cargo.lock` (MIT OR Apache-2.0), and YARD's features turn none of them off.
- Elements declare roles, names, and states with `.role(..)`, `.aria_label(..)`, `.aria_selected(..)`, `.aria_toggled(..)`, `.aria_value(..)`, and handle actions with `.on_a11y_action(..)`. A click and a focus come for free.

Zed's `ui` components already carry some of it: `Button` (role and its label), `IconButton` (role only), `Switch`, `ContextMenu`, `ListItem` (opt-in). Tabs, modals, text inputs, and tooltips have none yet.

This slice gives YARD's own UI kit and shell the roles and names a screen reader needs, and lets an extension label what it draws. The design follows Zed's `ui` crate, Electron and Chromium's ARIA mapping, Flutter's `Semantics`, and egui's AccessKit widgets. The sources are in the proposal.

## What an extension writes

```rust
// yard_ui components
IconButton::new("refresh", IconName::RotateCw).aria_label("Refresh")
TextInput::new(window, cx).label("Search").placeholder("Type to search")
ListItem::new("result-3").aria_role(Role::ListBoxOption).aria_label(title).aria_selected(selected)

// its own element
div().id("meter").role(gpui::Role::Meter).aria_label("Upload").aria_numeric_value(0.4)
```

- **Extensions use GPUI's API directly** for their own elements. `gpui` re-exports `Role`, `Toggled`, and `AccessibleAction`, and it is Apache-2.0. `yard_ui` adds no wrapper of its own.
- **An element enters the tree only with an id and a role.** An element without a role is laid out as before and is invisible to the screen reader. That is the right thing for decoration.
- **Ids are unique among siblings.** Once the tree is active, GPUI panics in a debug build on two siblings with one id, and drops one in a release build. A component drawn in a loop takes its id from the item, never from the call site alone.
- **An element that takes focus has an id and a role,** even a view's own container (the role `Group` will do). Otherwise GPUI reports focus on the whole window, and logs "focused element has no accessibility node" once. The shell's root is a `Group`.
- **One element per focus handle reports focus.** Two elements with a role that track one handle make GPUI set focus twice, and panic in a debug build. A component placed by the shell either:
  - leaves its node's focus to the shell, whose wrapper around the component tracks the handle as a `Group`;
  - or takes it by calling `yard_shell::focus_handle(cx, node)`, to handle its own actions. Then it tracks the handle on its own element with an id and a role, and the wrapper no longer tracks it.
  - It takes the handle when it is created or in its own render, never first inside a cached child (`Entity::cached`): a cached child renders after the wrapper has reported focus in that frame, so the focus would be reported twice.
  - `yard_shell::node_focus_handle(cx, node)` returns the same handle without taking it, to ask whether the node has focus or to focus it from code. The shell keeps reporting the focus.
- **A modal view puts its focus on an element with an id and a role.** `yard_ui::Modal` does it with `.track_focus(&handle)`. Otherwise GPUI reports focus on the whole window and logs it once.

## The UI kit (`yard_ui`)

Each change below is to a production part, as `close-guard` did. It is shown in the acceptance page and stamped again.

- **`yard_ui/button`:**
  - `Button` keeps the label it already gives.
  - `Button` and `IconButton` gain `aria_label`, `aria_description`, and `aria_expanded`, forwarded to Zed's `ButtonLike`.
  - An `IconButton` has no visible text, so a caller names it with `aria_label`.
- **`yard_ui/text_input`:**
  - The root element gets a stable id, the role `TextInput`, and the name from a new `.label(..)`, falling back to the placeholder, or empty.
  - It also exposes the placeholder and the current text as its value.
  - A screen reader's "set value" replaces the text as typing would: it emits `TextInputEvent::Changed` and moves the caret to the end. It runs inside a lifecycle group `text-input-set-value`. The same value again changes nothing, the caret included.
- **`yard_ui/tabs`:**
  - `TabBar` has the role `TabList`.
  - Each `Tab` has the role `Tab`, its label as its name, `selected`, and its position and count in the bar. The position is given to GPUI zero-based, since AccessKit's adapters add one; a screen reader says "1 of 2". The count is set where the adapters read it ([YARD#24](https://github.com/Riso-platform-factory/YARD/issues/24)).
  - The close button is named "Close <label>".
- **`yard_ui/list`:**
  - `List` has the role `List` when it is given an id with `.id(..)`. Without one it is a plain container, and its items are still in the tree. This keeps two lists drawn from one call site from colliding. A list without an id adds no id scope, so its rows share the enclosing one: a view that draws two lists with the same row ids gives the lists, or their parents, ids.
  - `ListItem` forwards Zed's `aria_role`, `aria_label`, and `aria_selected`. A list of choices uses `ListBoxOption`.
- **`yard_ui/modal` and `yard_ui/modal_surface`:**
  - A modal has the role `Dialog`, named by its headline, or by `.aria_label(..)` when it has none.
  - `Modal::track_focus(&handle)` puts the modal view's focus on the dialog itself.
  - `ModalSurface` has the role `Dialog` only when it is given an id with `.id(..)`, as `List`.
  - Its close button is named "Close".
- **`yard_ui/modal_layer`:** the layer's own focus element gets an id and the role `Group`, so focus on the layer is never reported as the whole window.
- **`yard_ui/toast_layer`:**
  - A toast's message (its icon and text) has the role `Alert` for a warning or an error, and `Status` for info, so a screen reader announces it. Its text is its name and its value.
  - Its buttons sit beside the message, not inside it, so they are not part of what is announced.
  - Its dismiss button is named "Dismiss".
- **`yard_ui/window_controls`:** the drawn window buttons on Linux and Windows are named "Minimize", "Maximize" or "Restore", and "Close". Each button's id includes its slot, so a layout that repeats a button does not collide. A screen reader's click runs the same lifecycle group as a mouse click.
- **`yard_ui/status_bar`:** the role `Status`.

The public API this adds is: `aria_label`, `aria_description`, and `aria_expanded` on `Button` and `IconButton`; `TextInput::label`; `ListItem::aria_role`, `aria_label`, and `aria_selected`; `List::id`; `ModalSurface::id`; `Modal::aria_label` and `Modal::track_focus`; and in `yard_shell`, `node_focus_handle`. The names YARD computes and the set-value handler stay private.

## The shell (`yard_shell/shell`)

- `set_window_title` also sets each open window's system title (`Window::set_window_title`), and a new window starts with it. An unchanged title does nothing, and several calls in one update set the system title once. GPUI names the tree's root after it, so a screen reader announces the window by the application's title, and so do the Window menu and the task switcher.
- The tab strip, the modal layer, and the toasts the shell draws use the kit above, so they need nothing more.

## The command palette (`command_palette/palette`)

- The palette is a `Dialog` named "Command palette".
- Its input is named "Command".
- Each command is a `ListBoxOption` named by the command, with `selected` on the highlighted one.

## Logging

- **The tree itself is per-frame work:** `yard_ui` keeps its `trace!` in `render` and adds nothing per frame.
- **A screen reader's action** that YARD handles itself runs in a group: `text-input-set-value{length}`, with `debug!("set")`. It never logs the text.
- **GPUI's own diagnostics** go to `yard.log` through `log`: "Accessibility activated" and "focused element has no accessibility node".

## Tests

GPUI's test window does not build the tree at `ddb90e5`, so a `TestAppContext` test cannot read it. The checks are:

- **Unit and acceptance tests**, written first by a worker that does not write the code:
  - the names YARD computes (tab close, window buttons, toast role by kind), as unit tests inside the parts;
  - `TextInput`'s set-value handler (the text, the event, the caret, and the same value with the caret moved), as a unit test;
  - the `text-input-set-value` group;
  - `set_window_title` reaching every open window's title, on the test platform.
- **The real tree, on each system:**
  - The test extension `examples/a11y_tour` (`A11Y_TOUR=1`) shows one of each component, and places a status bar item.
  - `examples/ui_gallery` (`UI_GALLERY=all`) is back in `review.rs`, as its README asks while a UI kit feature is under review.
  - Its **Dump tree** button writes `Window::debug_a11y_tree_json()` to `<logs>/a11y-tree.json` once a screen reader or an inspector has activated the tree.
  - On macOS, the lead also reads the tree through the Accessibility API, and sets the text field's value through it, as a screen reader does.
  - On the runners, YARD-testing reads it through UI Automation on Windows and AT-SPI on Linux, and compares the roles and names.

## Not in this slice

- **Disabled state:** GPUI's elements have no `aria_disabled` at `ddb90e5`, so a disabled button is announced as enabled. `TODO(accessibility)`, unblocked by a GPUI update that adds it.
- **The palette's highlighted command is not announced while typing.** Focus stays in the input, which is not an ancestor of the list, and GPUI's `aria_active_descendant` needs that. `TODO(accessibility)`, unblocked by restructuring the palette.
- **The text inside an input** (caret, selection, words) is not exposed. Zed has not done it for its editor either. On Linux this also means a text input has no AT-SPI text interface, so a screen reader cannot read or set its text there; macOS and Windows expose its value ([YARD#25](https://github.com/Riso-platform-factory/YARD/issues/25)).
- **Plain text.** A `Label` or `Headline` has no id and no role, so text drawn with it is not in the tree: a screen reader reads a control's name, but not a paragraph beside it or a status bar's text. Zed's `ui` labels have the same gap at `ddb90e5`. `TODO(accessibility)`, unblocked by giving labels stable ids without colliding in loops.
- **Headings, live regions other than toasts, and tooltips.**
- **A check that every focusable element has a role,** as a debug assertion. GPUI already logs it once per element.
