use gpui::{InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_keymap::load_user_keymap;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-keymap-user-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

gpui::actions!(keymap_test, [Ping, Pong]);

#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, gpui::Action)]
#[action(namespace = keymap_test)]
struct WithInput {
    value: String,
    times: u32,
}

/// Counts of each test action's handler calls.
#[derive(Clone)]
struct Counts {
    pings: Rc<Cell<usize>>,
    pongs: Rc<Cell<usize>>,
    inputs: Rc<RefCell<Vec<WithInput>>>,
}

impl Counts {
    fn new() -> Self {
        Self {
            pings: Rc::new(Cell::new(0)),
            pongs: Rc::new(Cell::new(0)),
            inputs: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

/// A view whose focused element has the `Shell` key context and counts a call
/// to each test action.
struct TestView {
    focus_handle: gpui::FocusHandle,
    counts: Counts,
}

impl gpui::Render for TestView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let pings = self.counts.pings.clone();
        let pongs = self.counts.pongs.clone();
        let inputs = self.counts.inputs.clone();
        gpui::div()
            .key_context("Shell")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_action(move |_: &Ping, _, _| pings.set(pings.get() + 1))
            .on_action(move |_: &Pong, _, _| pongs.set(pongs.get() + 1))
            .on_action(move |action: &WithInput, _, _| {
                inputs.borrow_mut().push(action.clone());
            })
    }
}

/// Binds `defaults` as the shell's keys, loads `path` as the user keymap over
/// them, then opens a window whose focused element has the `Shell` context.
/// Returns the window's context and how many bindings were bound.
fn load_user_keys<'a>(
    cx: &'a mut TestAppContext,
    counts: &Counts,
    defaults: Vec<gpui::KeyBinding>,
    path: &Path,
) -> (&'a mut gpui::VisualTestContext, usize) {
    let bound = cx.update(|cx| {
        cx.bind_keys(defaults);
        load_user_keymap(cx, path)
    });
    let (view, visual) = cx.add_window_view({
        let counts = counts.clone();
        move |_, cx| TestView {
            focus_handle: cx.focus_handle(),
            counts,
        }
    });
    let focus_handle = visual.update(|_, cx| view.read(cx).focus_handle.clone());
    visual.update(|window, cx| {
        window.focus(&focus_handle, cx);
        window.activate_window();
    });
    (visual, bound)
}

#[gpui::test]
fn a_user_key_runs_its_action_and_the_default_key_still_runs_it(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 1);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pings.get(), 1);
    // The default key still runs the action.
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 2);
    assert_eq!(counts.pongs.get(), 0);
}

#[gpui::test]
fn the_users_binding_replaces_the_default_on_the_same_keys(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"ctrl-a": "keymap_test::Pong"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 1);
    visual.simulate_keystrokes("ctrl-a");
    // The keystroke runs the user's action, not the default's.
    assert_eq!(counts.pongs.get(), 1);
    assert_eq!(counts.pings.get(), 0);
}

#[gpui::test]
fn null_disables_the_keystrokes_in_that_context(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"ctrl-a": null}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![
            gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell")),
            gpui::KeyBinding::new("ctrl-b", Pong, Some("Shell")),
        ],
        &path,
    );
    // The `null` entry is itself a bound binding.
    assert_eq!(bound, 1);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 0);
    assert_eq!(counts.pongs.get(), 0);
    // The removal is scoped to those keystrokes: the other key still works.
    visual.simulate_keystrokes("ctrl-b");
    assert_eq!(counts.pongs.get(), 1);
}

#[gpui::test]
fn unbind_removes_the_default_binding_of_that_action_for_those_keys(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"ctrl-a": ["zed::Unbind", "keymap_test::Ping"]}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![
            gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell")),
            gpui::KeyBinding::new("ctrl-b", Pong, Some("Shell")),
        ],
        &path,
    );
    assert_eq!(bound, 1);
    // `bindings_for_action` no longer lists the removed binding.
    let ping_bindings = visual.update(|_, cx| {
        cx.key_bindings()
            .borrow()
            .bindings_for_action(&Ping)
            .count()
    });
    assert_eq!(ping_bindings, 0);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 0);
    // Bindings for other actions are untouched.
    visual.simulate_keystrokes("ctrl-b");
    assert_eq!(counts.pongs.get(), 1);
}

#[gpui::test]
fn an_action_with_input_is_built_with_its_input(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"bindings": {"ctrl-i": ["keymap_test::WithInput", {"value": "hiya", "times": 3}]}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(cx, &counts, vec![], &path);
    assert_eq!(bound, 1);
    visual.simulate_keystrokes("ctrl-i");
    assert_eq!(
        counts.inputs.borrow().as_slice(),
        &[WithInput {
            value: "hiya".to_string(),
            times: 3,
        }]
    );
}

#[gpui::test]
fn each_bad_entry_is_skipped_and_the_others_load(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[
            {"context": "Shell &&", "bindings": {"ctrl-z": "keymap_test::Pong"}},
            42,
            {"bindings": {
                "cmd-+-x": "keymap_test::Ping",
                "ctrl-x": "keymap_test::Nope",
                "ctrl-c": ["keymap_test::WithInput", {"value": 1, "times": "x"}],
                "ctrl-a": "keymap_test::Ping"
            }},
            {"context": "Shell", "bindings": {"ctrl-p": "keymap_test::Pong"}}
        ]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(cx, &counts, vec![], &path);
    // Only `ctrl-a` and `ctrl-p` bound: a context that cannot be parsed skips
    // its section, a section that is not an object is skipped, and a binding
    // whose keystrokes, action, or action input cannot be read is skipped.
    assert_eq!(bound, 2);

    visual.simulate_keystrokes("ctrl-z");
    visual.simulate_keystrokes("ctrl-x");
    visual.simulate_keystrokes("ctrl-c");
    assert_eq!(counts.pings.get(), 0);
    assert_eq!(counts.pongs.get(), 0);
    assert!(counts.inputs.borrow().is_empty());

    // The good entries on either side of the bad ones still work.
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_keystrokes("ctrl-p");
    assert_eq!(counts.pings.get(), 1);
    assert_eq!(counts.pongs.get(), 1);
}

#[gpui::test]
fn a_file_that_is_not_json_or_not_an_array_binds_nothing(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let not_json = temp.path().join("not_json.json");
    fs::write(&not_json, "not json").unwrap();
    let not_array = temp.path().join("not_array.json");
    fs::write(
        &not_array,
        r#"{"bindings": {"ctrl-b": "keymap_test::Pong"}}"#,
    )
    .unwrap();

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &not_json,
    );
    assert_eq!(bound, 0);
    let bound = visual.update(|_, cx| load_user_keymap(cx, &not_array));
    assert_eq!(bound, 0);
    // Only the default binding is in the keymap.
    let total = visual.update(|_, cx| cx.key_bindings().borrow().bindings().len());
    assert_eq!(total, 1);
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_keystrokes("ctrl-b");
    assert_eq!(counts.pings.get(), 1);
    assert_eq!(counts.pongs.get(), 0);
}

#[gpui::test]
fn no_file_binds_nothing_and_returns_zero(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    assert!(!path.exists());

    let counts = Counts::new();
    let (visual, bound) = load_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 0);
    let total = visual.update(|_, cx| cx.key_bindings().borrow().bindings().len());
    assert_eq!(total, 1);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 1);
    assert_eq!(counts.pongs.get(), 0);
}
