#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_layout::{Operation, decode};
use riso_shell::dev as shell;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
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
            "riso-shell-new-window-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Every test in this file shares one process-wide diagnostic logger and runs
// under one lock, so a test never sees another test's records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        RECORDS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((record.level(), record.args().to_string()));
    }
    fn flush(&self) {}
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    // A panicked test poisons the shared state; recover so the next test
    // still reports its own verdict.
    let guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    guard
}

fn records() -> Vec<(log::Level, String)> {
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// A card fills the zone its container gives it and is found as `CARD-<node>`.
struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

// `host` registers `tour.card`, a leaf component.
fn extension(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host
    })
}

// The windows the opener opened, oldest first.
type Opened = Rc<RefCell<Vec<gpui::WindowHandle<shell::Shell>>>>;

// The opener the composition root gives the shell opens a window as the
// startup opens the main one. The tests' opener counts its calls, keeps the
// window it opened, and activates the window, as the OS does for a window it
// puts on screen.
fn opener(app: &mut TestAppContext) -> (Opened, Rc<Cell<usize>>) {
    let opened = Opened::default();
    let calls = Rc::new(Cell::new(0));
    let (recorded, counted) = (opened.clone(), calls.clone());
    app.update(move |cx| {
        shell::set_window_opener(cx, move |cx| {
            counted.set(counted.get() + 1);
            let window = cx
                .open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap();
            window
                .update(cx, |_, window, _| window.activate_window())
                .unwrap();
            recorded.borrow_mut().push(window);
        });
    });
    (opened, calls)
}

// Draw without forcing a refresh: operations must schedule it themselves.
fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

// The window with the keyboard focus is the one calls act on.
fn activate(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
}

fn window_id(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> u64 {
    window
        .update(app, |_, window, _| {
            window.window_handle().window_id().as_u64()
        })
        .unwrap()
}

fn active_window_id(app: &mut TestAppContext) -> Option<gpui::WindowId> {
    app.update(|cx| cx.active_window().map(|window| window.window_id()))
}

// `riso::NewWindow` dispatched on `window`.
fn dispatch_new_window(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    app.dispatch_action(window.into(), shell::NewWindow);
}

// The ids `inspect` lists for the window the calls act on.
fn inspected(app: &mut TestAppContext) -> Vec<String> {
    app.update(|cx| {
        shell::inspect(cx)
            .iter()
            .map(|info| info.id.to_string())
            .collect()
    })
}

fn hidden(app: &mut TestAppContext, node: &str) -> bool {
    app.update(|cx| shell::is_hidden(cx, node))
}

fn shown(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> String {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            shell::shown_layout(cx, window).to_string()
        })
        .unwrap()
}

fn saved_ops(layout_path: &std::path::Path) -> Vec<Operation> {
    decode(&fs::read_to_string(layout_path).unwrap()).unwrap()
}

fn is_hide(operation: &Operation, node: &str) -> bool {
    matches!(operation, Operation::SetHidden { node: id, hidden: true } if id.as_str() == node)
}

// `riso::NewWindow` calls the given opener once. The window it opens exists,
// shows the default layout with the named layout `riso.default`, an empty
// user layer of its own, and no zoom, and is the active window, so the next
// calls act on it.
#[gpui::test]
fn new_window_calls_the_opener_once_and_the_new_window_is_active(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    let (opened, calls) = opener(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "root", "children");
        }
    });
    let first = open_and_draw(&mut app);
    activate(first, &mut app);

    dispatch_new_window(first, &mut app);

    assert_eq!(calls.get(), 1, "the opener ran {} times", calls.get());
    assert_eq!(app.update(|cx| cx.windows().len()), 2);
    let second = opened.borrow().first().copied().unwrap();
    let second_id = window_id(second, &mut app);
    assert_eq!(
        active_window_id(&mut app),
        Some(gpui::WindowId::from(second_id)),
        "the new window did not become the active window"
    );

    // The new window shows the default layout, with an empty user layer of
    // its own and no zoom.
    assert_eq!(inspected(&mut app), ["root", "a", "b", "c"]);
    assert_eq!(shown(second, &mut app), "riso.default");
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert_eq!(app.update(|cx| shell::zoomed(cx)), None);

    // The next calls act on the new window, not on the window it took the
    // focus from.
    app.update(|cx| host.set_hidden(cx, "a", true));
    assert!(hidden(&mut app, "a"));
    activate(first, &mut app);
    assert!(!hidden(&mut app, "a"), "the hide reached the first window");
    activate(second, &mut app);
    assert!(hidden(&mut app, "a"));
    assert!(errors().is_empty(), "{:?}", errors());
}

// A window opened by `riso::NewWindow` is rearranged on its own: the two
// windows keep different trees and different user layers, and the saved file
// holds only the main window's operations.
#[gpui::test]
fn a_window_opened_by_new_window_keeps_its_own_layer_out_of_the_saved_file(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    let (opened, calls) = opener(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "root", "children");
        }
    });
    let main = open_and_draw(&mut app);
    dispatch_new_window(main, &mut app);
    assert_eq!(calls.get(), 1, "the opener ran {} times", calls.get());
    let second = opened.borrow().first().copied().unwrap();

    // The windows are rearranged differently.
    activate(main, &mut app);
    app.update(|cx| host.set_hidden(cx, "a", true));
    activate(second, &mut app);
    app.update(|cx| {
        host.set_hidden(cx, "b", true);
        host.move_node(cx, "c", "root", "children", 0);
    });

    // Each keeps its own arrangement and its own layer.
    assert_eq!(inspected(&mut app), ["root", "c", "a", "b"]);
    activate(main, &mut app);
    assert_eq!(inspected(&mut app), ["root", "a", "b", "c"]);
    assert!(hidden(&mut app, "a"));
    assert!(!hidden(&mut app, "b"));
    activate(second, &mut app);
    assert!(hidden(&mut app, "b"));
    assert!(!hidden(&mut app, "a"));

    // The saved file holds only the main window's operations.
    let ops = saved_ops(&layout_path);
    assert_eq!(ops.len(), 1, "{ops:?}");
    assert!(is_hide(&ops[0], "a"), "{ops:?}");

    // Another operation in the main window is saved; the second window's
    // operations still never reach the file.
    activate(main, &mut app);
    app.update(|cx| host.set_hidden(cx, "c", true));
    let ops = saved_ops(&layout_path);
    assert_eq!(ops.len(), 2, "{ops:?}");
    assert!(is_hide(&ops[0], "a"), "{ops:?}");
    assert!(is_hide(&ops[1], "c"), "{ops:?}");
    assert!(errors().is_empty(), "{:?}", errors());
}

// `keys` is the one binding `bind_default_keys` gives `riso::NewWindow`, in
// the Shell context.
fn new_window_is_bound(cx: &mut TestAppContext, keys: &str) {
    let _guard = lock();
    cx.update(|cx| {
        shell::bind_default_keys(cx);
        let os = cx.platform_info().os;
        let context = gpui::KeyBindingContextPredicate::parse("Shell").unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bindings: Vec<_> = keymap.bindings_for_action(&shell::NewWindow).collect();
        assert_eq!(bindings.len(), 1, "exactly one binding for riso::NewWindow");
        let bound: Vec<String> = bindings[0]
            .keystrokes()
            .iter()
            .map(|keystroke| keystroke.unparse(os))
            .collect();
        assert_eq!(bound, [keys]);
        assert_eq!(
            bindings[0].predicate().as_deref(),
            Some(&context),
            "the riso::NewWindow binding is not in the Shell context"
        );
    });
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn bind_default_keys_binds_new_window_to_cmd_shift_n_in_the_shell_context(cx: &mut TestAppContext) {
    new_window_is_bound(cx, "cmd-shift-n");
}

#[cfg(not(target_os = "macos"))]
#[gpui::test]
fn bind_default_keys_binds_new_window_to_ctrl_shift_n_in_the_shell_context(
    cx: &mut TestAppContext,
) {
    new_window_is_bound(cx, "ctrl-shift-n");
}

// Without an opener the action is refused with one WARN, and nothing opens.
#[gpui::test]
fn without_an_opener_new_window_warns_once_and_opens_nothing(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mark = records().len();
    dispatch_new_window(window, &mut app);
    assert_eq!(
        app.update(|cx| cx.windows().len()),
        1,
        "a window opened without an opener"
    );
    let warnings: Vec<String> = records()[mark..]
        .iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, message)| message.clone())
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "the refusal did not warn once: {warnings:?}"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}
