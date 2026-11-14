#![cfg(feature = "dev")]

use gpui::{InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_keymap::{dev::watch_user_keymap, load_user_keymap};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-keymap-reload-{}-{}-{nanos:x}",
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

/// Counts of each test action's handler calls.
#[derive(Clone)]
struct Counts {
    pings: Rc<Cell<usize>>,
    pongs: Rc<Cell<usize>>,
}

impl Counts {
    fn new() -> Self {
        Self {
            pings: Rc::new(Cell::new(0)),
            pongs: Rc::new(Cell::new(0)),
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
        gpui::div()
            .key_context("Shell")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_action(move |_: &Ping, _, _| pings.set(pings.get() + 1))
            .on_action(move |_: &Pong, _, _| pongs.set(pongs.get() + 1))
    }
}

/// Binds `defaults` as the shell's keys, loads `path` as the user keymap over
/// them, starts watching the file, and opens a window whose focused element
/// has the `Shell` context. Returns the window's context and how many bindings
/// the first load bound.
fn watch_user_keys<'a>(
    cx: &'a mut TestAppContext,
    counts: &Counts,
    defaults: Vec<gpui::KeyBinding>,
    path: &Path,
) -> (&'a mut gpui::VisualTestContext, usize) {
    let bound = cx.update(|cx| {
        cx.bind_keys(defaults);
        let bound = load_user_keymap(cx, path);
        watch_user_keymap(cx, path.to_path_buf());
        bound
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

/// Advances the test clock one second, lets the once-a-second check run, and
/// draws a frame.
fn a_second_passes(executor: &gpui::BackgroundExecutor, visual: &mut gpui::VisualTestContext) {
    executor.advance_clock(Duration::from_secs(1));
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn a_binding_the_file_gains_runs_its_action_a_second_later(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    assert!(!path.exists());

    let counts = Counts::new();
    let executor = cx.executor();
    let (visual, bound) = watch_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 0);

    // The keystroke runs nothing until the file binds it.
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pings.get(), 0);

    // The file is created with the binding: a second later it works.
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping"}}]"#,
    )
    .unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pings.get(), 1);

    // An edit adds a binding: a second later it works too.
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping", "cmd-alt-q": "keymap_test::Pong"}}]"#,
    )
    .unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-alt-q");
    assert_eq!(counts.pongs.get(), 1);
    // The earlier binding and the default key are still bound.
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pings.get(), 2);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 3);
}

#[gpui::test]
fn a_binding_the_file_loses_runs_nothing_and_the_default_keys_still_work(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Pong", "ctrl-a": "keymap_test::Pong"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let executor = cx.executor();
    let (visual, bound) = watch_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 2);
    visual.simulate_keystrokes("cmd-shift-z");
    visual.simulate_keystrokes("ctrl-a");
    // `ctrl-a` runs the user's action, not the default's.
    assert_eq!(counts.pongs.get(), 2);
    assert_eq!(counts.pings.get(), 0);

    // The file loses both bindings.
    fs::write(&path, "[]").unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pongs.get(), 2);
    // `ctrl-a` is the default's again.
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 1);
}

#[gpui::test]
fn a_file_that_is_not_valid_json_leaves_the_defaults_until_a_good_edit_binds_again(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Pong"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let executor = cx.executor();
    let (visual, bound) = watch_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 1);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pongs.get(), 1);

    // A broken edit leaves the defaults alone.
    fs::write(&path, "not json").unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pongs.get(), 1);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 1);

    // A good edit binds the user's keys again.
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping"}}]"#,
    )
    .unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pings.get(), 2);
}

#[gpui::test]
fn deleting_the_file_leaves_the_defaults_alone(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Pong", "ctrl-a": "keymap_test::Pong"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let executor = cx.executor();
    let (visual, bound) = watch_user_keys(
        cx,
        &counts,
        vec![gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell"))],
        &path,
    );
    assert_eq!(bound, 2);
    visual.simulate_keystrokes("ctrl-a");
    // `ctrl-a` runs the user's action, not the default's.
    assert_eq!(counts.pongs.get(), 1);

    fs::remove_file(&path).unwrap();
    a_second_passes(&executor, visual);
    visual.simulate_keystrokes("cmd-shift-z");
    assert_eq!(counts.pongs.get(), 1);
    // `ctrl-a` is the default's again.
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 1);
}

#[gpui::test]
fn a_check_that_finds_nothing_new_binds_nothing_again(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("keymap.json");
    fs::write(
        &path,
        r#"[{"context": "Shell", "bindings": {"ctrl-p": "keymap_test::Pong"}}]"#,
    )
    .unwrap();

    let counts = Counts::new();
    let executor = cx.executor();
    let (visual, bound) = watch_user_keys(
        cx,
        &counts,
        vec![
            gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell")),
            gpui::KeyBinding::new("ctrl-b", Pong, Some("Shell")),
        ],
        &path,
    );
    assert_eq!(bound, 1);
    let count = visual.update(|_, cx| cx.key_bindings().borrow().bindings().count());
    for _ in 0..3 {
        a_second_passes(&executor, visual);
    }
    let after = visual.update(|_, cx| cx.key_bindings().borrow().bindings().count());
    assert_eq!(count, after);
    // The keys are as they were.
    visual.simulate_keystrokes("ctrl-p");
    assert_eq!(counts.pongs.get(), 1);
    visual.simulate_keystrokes("ctrl-a");
    assert_eq!(counts.pings.get(), 1);
}
