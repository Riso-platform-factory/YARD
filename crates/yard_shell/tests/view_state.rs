use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_layout::NodeId;
use yard_shell as shell;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-view-state-{}-{}-{nanos:x}",
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

// Every test in this file shares one process-wide diagnostic logger and runs
// under one lock, so a test never sees another test's records, and a test
// that captures the lifecycle trace never runs beside another that reaches
// the same lines.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
            || metadata.target().starts_with("yard_state")
            || metadata.target().starts_with("yard_paths")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((record.level(), record.args().to_string()));
        }
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

fn warnings() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, message)| message)
        .collect()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// The groups the code opens and the events it writes, so a test can count
// the `state-save` groups — one is one write of the file — and read the
// `debug` lines a run logs.
#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
}

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let id = captured.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell")
            || target.starts_with("yard_state")
            || target.starts_with("yard_paths"))
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
                .pop(),
            Some(id.into_u64())
        );
    }
}

impl Trace {
    fn mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .len()
    }
    // The lines since `mark`, without the quoting Debug fields add.
    fn since(&self, mark: usize) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines[mark..]
            .iter()
            .map(|line| line.replace('"', ""))
            .collect()
    }
    // The `state-save` groups opened so far: one group is one write of the
    // file.
    fn state_saves(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .values()
            .filter(|label| label.starts_with("state-save"))
            .count()
    }
}

// Every test runs under the one lock and the one subscriber, so no thread
// reaches the shell's log lines without them.
fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
    assert!(
        trace
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .is_empty()
    );
}

// A card fills the zone its container gives it and is found as `CARD-<node>`;
// `empty` is what it reports through `Emptiable`.
struct Card {
    node: String,
    empty: bool,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

impl shell::Emptiable for Card {
    fn is_empty(&self, _: &gpui::App) -> bool {
        self.empty
    }
}

// The view each node's card got in each window: window ID, then node ID.
type Views = Rc<RefCell<BTreeMap<u64, BTreeMap<String, gpui::WeakEntity<Card>>>>>;

// The nodes whose cards report empty from their creation.
type Starts = Rc<RefCell<BTreeSet<String>>>;

// `host` registers `tour.card`, which can be empty, and places a row of the
// card `a` and the `right` tabs container over `b`, `c`, `d`.
fn extension(app: &mut TestAppContext) -> (shell::Extension, Views, Starts) {
    let views = Views::default();
    let starts = Starts::default();
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = views.clone();
        let empty_at_start = starts.clone();
        host.register(cx, "tour.card", &[], move |node, _, window, cx| {
            let window_id = window.window_handle().window_id().as_u64();
            let card = cx.new(|_| Card {
                node: node.to_string(),
                empty: empty_at_start.borrow().contains(node.as_str()),
            });
            created
                .borrow_mut()
                .entry(window_id)
                .or_default()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        host.register_empty::<Card>(cx, "tour.card");
        host.place_root(cx, "root", "yard.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "right", "yard.tabs", "root", "children");
        for node in ["b", "c", "d"] {
            host.place(cx, node, "tour.card", "right", "children");
        }
        host
    });
    (host, views, starts)
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
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
    draw(window, app);
    window
}

// The window the startup opens for `saved`: a root built by
// `new_session_shell` on a window with the saved bounds and display.
fn reopen(
    app: &mut TestAppContext,
    saved: &shell::SessionWindow,
) -> gpui::WindowHandle<shell::Shell> {
    app.update(|cx| {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: saved.bounds,
                display_id: saved.display,
                ..Default::default()
            },
            |window, cx| shell::new_session_shell(window, cx, saved),
        )
        .unwrap()
    })
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

fn card(views: &Views, window: u64, node: &str) -> gpui::Entity<Card> {
    views.borrow()[&window][node]
        .upgrade()
        .unwrap_or_else(|| panic!("{node} has a view in window {window}"))
}

// The view changes what it reports and notifies; the next frame lays the
// window out again.
fn set_empty(app: &mut TestAppContext, views: &Views, window: u64, node: &str, empty: bool) {
    let card = card(views, window, node);
    app.update(|cx| {
        card.update(cx, |card, cx| {
            card.empty = empty;
            cx.notify();
        });
    });
}

// The changes made so far reach the state file: deferred work runs, the
// 300 ms wait of a save passes, and the write itself finishes.
fn settle(app: &mut TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(400));
    app.run_until_parked();
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

fn selected(app: &mut TestAppContext, tabs: &str) -> Option<NodeId> {
    app.update(|cx| shell::selected_tab(cx, tabs))
}

fn zoomed(app: &mut TestAppContext) -> Option<String> {
    app.update(|cx| shell::zoomed(cx))
}

fn state_json(file: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

// The windows of a saved session file, as feature 10's file holds them.
fn write_session_file(file: &Path, windows: usize) {
    let window = json!({
        "display": null,
        "bounds": {"state": "windowed", "x": 0., "y": 0., "width": 800., "height": 600.},
        "layout": "yard.default",
        "layers": {"yard.default": []},
    });
    fs::write(
        file,
        json!({"version": 1, "windows": vec![window; windows]}).to_string(),
    )
    .unwrap();
}

// The tab `d` selected, quit, a new run with the file: `d` is selected. The
// file holds the tab under the container's ID with the `yard.tabs`
// component, and the zoom under `window` with `yard.window`.
#[gpui::test]
fn a_selected_tab_quits_and_is_selected_at_the_next_run(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, _views, _starts) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click(&mut visual, "TAB-d");
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "right"), Some(id("d")));
        app.quit();
        app.run_until_parked();

        let json = state_json(&file);
        let windows = json["windows"]
            .as_array()
            .unwrap_or_else(|| panic!("no `windows` in {json}"));
        assert_eq!(windows.len(), 1, "{json}");
        assert_eq!(windows[0]["right"]["component"], "yard.tabs", "{windows:?}");
        assert_eq!(
            windows[0]["right"]["state"],
            json!({"selected": "d"}),
            "{windows:?}"
        );
        assert_eq!(
            windows[0]["window"]["component"], "yard.window",
            "{windows:?}"
        );
        assert_eq!(
            windows[0]["window"]["state"],
            json!({"zoomed": null}),
            "{windows:?}"
        );

        let mut restart = cx.new_app();
        let (_host, _views, _starts) = extension(&mut restart);
        restart.update(|cx| shell::set_state_file(cx, file.clone()));
        let _window = open_and_draw(&mut restart);
        assert_eq!(selected(&mut restart, "right"), Some(id("d")));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A node zoomed, quit, a new run: it is zoomed — the saved zoom replaces
// the zoom the extension set when it started. Zoomed out, quit, a new run:
// nothing is zoomed, though the extension zooms a node again — the saved
// `null` is what the user last saw.
#[gpui::test]
fn a_zoomed_node_and_a_zoomed_out_both_survive_a_run(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("state.json");

        // The extension zooms `a` when it starts; `d` is zoomed and the
        // application quits.
        let mut app = cx.new_app();
        let (host, _views, _starts) = extension(&mut app);
        app.update(|cx| {
            shell::set_state_file(cx, file.clone());
            host.set_zoomed(cx, "a");
        });
        let _window = open_and_draw(&mut app);
        assert_eq!(zoomed(&mut app).as_deref(), Some("a"));
        app.update(|cx| host.set_zoomed(cx, "d"));
        assert_eq!(zoomed(&mut app).as_deref(), Some("d"));
        app.quit();
        app.run_until_parked();
        let json = state_json(&file);
        assert_eq!(
            json["windows"][0]["window"]["state"],
            json!({"zoomed": "d"}),
            "{json}"
        );

        // The saved `d` replaces the extension's `a`.
        let mut app = cx.new_app();
        let (host, _views, _starts) = extension(&mut app);
        app.update(|cx| {
            shell::set_state_file(cx, file.clone());
            host.set_zoomed(cx, "a");
        });
        let window = open_and_draw(&mut app);
        assert_eq!(
            zoomed(&mut app).as_deref(),
            Some("d"),
            "the saved zoom did not replace the extension's"
        );
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(
            visual.debug_bounds("CARD-d").is_some(),
            "the zoomed node fills the window"
        );
        assert!(visual.debug_bounds("CARD-a").is_none());
        app.update(|cx| host.zoom_out(cx));
        assert_eq!(zoomed(&mut app), None);
        app.quit();
        app.run_until_parked();
        let json = state_json(&file);
        assert_eq!(
            json["windows"][0]["window"]["state"],
            json!({"zoomed": null}),
            "{json}"
        );

        // The saved `null` replaces the extension's `a` too.
        let mut app = cx.new_app();
        let (host, _views, _starts) = extension(&mut app);
        app.update(|cx| {
            shell::set_state_file(cx, file.clone());
            host.set_zoomed(cx, "a");
        });
        let _window = open_and_draw(&mut app);
        assert_eq!(
            zoomed(&mut app),
            None,
            "the saved `null` did not clear the extension's zoom"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A saved tab that is hidden or empty at the next start has no tab then:
// the first tab is selected, with the `debug` `saved tab has no tab
// {tabs, tab}`.
#[gpui::test]
fn a_saved_tab_that_is_hidden_or_empty_selects_the_first_tab(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        for hide in [true, false] {
            let temp = TempDir::new();
            let file = temp.path().join("state.json");
            fs::write(
                &file,
                json!({
                    "version": 1,
                    "windows": [{
                        "right": {"component": "yard.tabs", "state": {"selected": "d"}},
                    }],
                })
                .to_string(),
            )
            .unwrap();
            let mut app = cx.new_app();
            let (host, _views, starts) = extension(&mut app);
            if hide {
                app.update(|cx| host.set_hidden(cx, "d", true));
            } else {
                starts.borrow_mut().insert("d".to_string());
            }
            app.update(|cx| shell::set_state_file(cx, file.clone()));
            let mark = trace.mark();
            let _window = open_and_draw(&mut app);
            assert_eq!(
                selected(&mut app, "right"),
                Some(id("b")),
                "the first tab is not selected (hide={hide})"
            );
            let lines = trace.since(mark);
            assert!(
                lines.iter().any(|line| {
                    line.contains("DEBUG")
                        && line.contains("saved tab has no tab")
                        && line.contains("tabs=right")
                        && line.contains("tab=d")
                }),
                "missing `saved tab has no tab {{tabs, tab}}` (hide={hide}): {lines:#?}"
            );
            assert!(errors().is_empty(), "{:?}", errors());
        }
    });
}

// A saved zoom of a node that is hidden or outside the body at the next
// start is not given: nothing is zoomed, with the `debug` `saved zoom not
// given {node}`.
#[gpui::test]
fn a_saved_zoom_of_a_node_hidden_or_outside_the_body_is_not_given(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        for outside_body in [false, true] {
            // `d` is hidden, `item` sits in the status bar, outside the body.
            let saved = if outside_body { "item" } else { "d" };
            let temp = TempDir::new();
            let file = temp.path().join("state.json");
            fs::write(
                &file,
                json!({
                    "version": 1,
                    "windows": [{
                        "window": {"component": "yard.window", "state": {"zoomed": saved}},
                    }],
                })
                .to_string(),
            )
            .unwrap();
            let mut app = cx.new_app();
            let (host, _views, _starts) = extension(&mut app);
            app.update(|cx| {
                if outside_body {
                    host.place(cx, "item", "tour.card", "status_bar", "left");
                } else {
                    host.set_hidden(cx, "d", true);
                }
                shell::set_state_file(cx, file.clone());
            });
            let mark = trace.mark();
            let _window = open_and_draw(&mut app);
            assert_eq!(zoomed(&mut app), None, "{saved} must not be zoomed");
            let lines = trace.since(mark);
            assert!(
                lines.iter().any(|line| {
                    line.contains("DEBUG")
                        && line.contains("saved zoom not given")
                        && line.contains(&format!("node={saved}"))
                }),
                "missing `saved zoom not given {{node}}` for {saved}: {lines:#?}"
            );
            assert!(errors().is_empty(), "{:?}", errors());
        }
    });
}

// Each window keeps its own selection and zoom, saved in the session's
// order — the main window first — and the window opened i-th from the
// session gets the i-th window's values back.
#[gpui::test]
fn two_windows_keep_their_own_selections_and_zooms(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("state.json");
        let session_file = temp.path().join("session.json");
        let mut app = cx.new_app();
        let (host, _views, _starts) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let first = open_and_draw(&mut app);
        activate(first, &mut app);
        let mut visual1 = gpui::VisualTestContext::from_window(first.into(), &app);
        click(&mut visual1, "TAB-c");
        app.update(|cx| host.set_zoomed(cx, "a"));
        let second = open_and_draw(&mut app);
        activate(second, &mut app);
        let mut visual2 = gpui::VisualTestContext::from_window(second.into(), &app);
        click(&mut visual2, "TAB-d");
        app.update(|cx| host.set_zoomed(cx, "b"));
        assert_eq!(selected(&mut app, "right"), Some(id("d")));
        assert_eq!(zoomed(&mut app).as_deref(), Some("b"));
        activate(first, &mut app);
        assert_eq!(selected(&mut app, "right"), Some(id("c")));
        assert_eq!(zoomed(&mut app).as_deref(), Some("a"));
        app.quit();
        app.run_until_parked();

        // The file holds both windows in the session's order: the first
        // window, the main one, first.
        let json = state_json(&file);
        let windows = json["windows"]
            .as_array()
            .unwrap_or_else(|| panic!("no `windows` in {json}"));
        assert_eq!(windows.len(), 2, "{json}");
        assert_eq!(
            windows[0]["right"]["state"],
            json!({"selected": "c"}),
            "{windows:?}"
        );
        assert_eq!(
            windows[0]["window"]["state"],
            json!({"zoomed": "a"}),
            "{windows:?}"
        );
        assert_eq!(
            windows[1]["right"]["state"],
            json!({"selected": "d"}),
            "{windows:?}"
        );
        assert_eq!(
            windows[1]["window"]["state"],
            json!({"zoomed": "b"}),
            "{windows:?}"
        );

        // A new run from saved files: two windows open from the session, in
        // order, and each gets its own selection and zoom back.
        write_session_file(&session_file, 2);
        let mut restart = cx.new_app();
        let (_host, _views, _starts) = extension(&mut restart);
        restart.update(|cx| shell::set_state_file(cx, file.clone()));
        let saved = restart
            .update(|cx| shell::saved_session(cx, &session_file))
            .expect("a saved session");
        assert_eq!(saved.len(), 2, "{saved:?}");
        let first = reopen(&mut restart, &saved[0]);
        let second = reopen(&mut restart, &saved[1]);
        draw(first, &mut restart);
        draw(second, &mut restart);
        activate(first, &mut restart);
        assert_eq!(selected(&mut restart, "right"), Some(id("c")));
        assert_eq!(zoomed(&mut restart).as_deref(), Some("a"));
        activate(second, &mut restart);
        assert_eq!(selected(&mut restart, "right"), Some(id("d")));
        assert_eq!(zoomed(&mut restart).as_deref(), Some("b"));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// Selecting a tab by a click and by an action, and zooming in and out, each
// lead to one save after the wait; a selection the shell moves by itself —
// the selected tab became empty — does not: the next save takes what is
// shown then.
#[gpui::test]
fn selecting_and_zooming_each_save_once_but_a_shell_moved_selection_does_not(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (host, views, _starts) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let window_id = window_id(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        settle(&mut app);
        let mut saves = trace.state_saves();

        // A click on `c`'s tab selects it and saves once after the wait.
        click(&mut visual, "TAB-c");
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "right"), Some(id("c")));
        settle(&mut app);
        saves += 1;
        assert_eq!(trace.state_saves(), saves, "a click did not save once");
        assert_eq!(
            state_json(&file)["windows"][0]["right"]["state"],
            json!({"selected": "c"})
        );

        // An action of the keymap engine does the same: `c`'s card has the
        // focus, so the next tab is `d`.
        click(&mut visual, "CARD-c");
        draw(window, &mut app);
        visual.dispatch_action(shell::SelectNextTab);
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "right"), Some(id("d")));
        settle(&mut app);
        saves += 1;
        assert_eq!(trace.state_saves(), saves, "an action did not save once");

        // Zooming in, then out, each save once.
        app.update(|cx| host.set_zoomed(cx, "a"));
        settle(&mut app);
        saves += 1;
        assert_eq!(trace.state_saves(), saves, "zooming in did not save once");
        assert_eq!(
            state_json(&file)["windows"][0]["window"]["state"],
            json!({"zoomed": "a"})
        );
        app.update(|cx| host.zoom_out(cx));
        settle(&mut app);
        saves += 1;
        assert_eq!(trace.state_saves(), saves, "zooming out did not save once");

        // `d` becomes empty: its tab drops and the shell moves the
        // selection to the first shown tab by itself — no save comes, and
        // the file keeps the last saved selection.
        set_empty(&mut app, &views, window_id, "d", true);
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "right"), Some(id("b")));
        settle(&mut app);
        assert_eq!(
            trace.state_saves(),
            saves,
            "a selection the shell moved by itself saved the file"
        );
        assert_eq!(
            state_json(&file)["windows"][0]["right"]["state"],
            json!({"selected": "d"}),
            "the file changed though no save ran"
        );

        // The next save takes what is shown then: quitting writes `b`.
        app.quit();
        app.run_until_parked();
        assert_eq!(
            state_json(&file)["windows"][0]["right"]["state"],
            json!({"selected": "b"})
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A state that cannot be read — not an object, or a value that is not a
// string or `null` — is a `WARN`, and the default is kept; a state that can
// be read still applies beside it.
#[gpui::test]
fn a_state_that_cannot_be_read_warns_and_keeps_the_default(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        for (right, window, expected_tab, expected_zoom) in [
            (json!("oops"), json!({"zoomed": "a"}), "b", Some("a")),
            (
                json!({"selected": 7}),
                json!({"zoomed": "a"}),
                "b",
                Some("a"),
            ),
            (json!({"selected": "d"}), json!({"zoomed": true}), "d", None),
        ] {
            let temp = TempDir::new();
            let file = temp.path().join("state.json");
            fs::write(
                &file,
                json!({
                    "version": 1,
                    "windows": [{
                        "right": {"component": "yard.tabs", "state": right},
                        "window": {"component": "yard.window", "state": window},
                    }],
                })
                .to_string(),
            )
            .unwrap();
            let mut app = cx.new_app();
            let (_host, _views, _starts) = extension(&mut app);
            app.update(|cx| shell::set_state_file(cx, file.clone()));
            let warnings_before = warnings().len();
            let _window = open_and_draw(&mut app);
            assert!(
                warnings().len() > warnings_before,
                "an unreadable view state did not warn: {right} / {window}"
            );
            assert_eq!(selected(&mut app, "right"), Some(id(expected_tab)));
            assert_eq!(zoomed(&mut app).as_deref(), expected_zoom);
        }
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
