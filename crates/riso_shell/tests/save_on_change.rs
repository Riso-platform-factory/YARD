use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell as shell;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex, Once},
    time::Duration,
};

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
        metadata.target().starts_with("riso_shell")
            || metadata.target().starts_with("riso_state")
            || metadata.target().starts_with("riso_paths")
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

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// The groups the code opens, so a test can count the `state-save` groups:
// one group is one write of the file.
#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
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
    fn event(&self, _: &tracing::Event<'_>) {}
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
// its `count` is the state it gives the shell as JSON and takes back.
struct Card {
    node: String,
    count: u64,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

impl shell::NodeState for Card {
    fn save_state(&self, _: &gpui::App) -> serde_json::Value {
        json!({"count": self.count})
    }
    fn restore_state(
        &mut self,
        state: serde_json::Value,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) {
        if let Some(count) = state.get("count").and_then(serde_json::Value::as_u64) {
            self.count = count;
        }
    }
}

// The view each node's card got in each window: window ID, then node ID.
type Views = Rc<RefCell<BTreeMap<u64, BTreeMap<String, gpui::WeakEntity<Card>>>>>;

// `host` registers `tour.card` with its state, and places the default
// `riso.row` of three cards.
fn extension(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let host = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, window, cx| {
            let window_id = window.window_handle().window_id().as_u64();
            let card = cx.new(|_| Card {
                node: node.to_string(),
                count: 0,
            });
            created
                .borrow_mut()
                .entry(window_id)
                .or_default()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        host.register_state::<Card>(cx, "tour.card");
        host.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "root", "children");
        }
        host
    });
    (host, views)
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

fn count(views: &Views, app: &mut TestAppContext, window: u64, node: &str) -> u64 {
    let card = card(views, window, node);
    app.update(|cx| card.read(cx).count)
}

// A change as the card makes one: the state moves, and the component says it
// changed.
fn change(views: &Views, app: &mut TestAppContext, window: u64, node: &str, count: u64) {
    let card = card(views, window, node);
    app.update(|cx| {
        card.update(cx, |card, _| card.count = count);
        shell::state_changed(cx);
    });
}

// The changes made so far reach the state file: deferred work runs, the
// 300 ms wait of a save passes, and the write itself finishes.
fn settle(app: &mut TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(400));
    app.run_until_parked();
}

fn saved_count(file: &Path, node: &str) -> u64 {
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    json["windows"][0][node]["state"]["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("no count for {node} in {json}"))
}

// Ten changes 10 ms apart are one burst: the file is written once, when no
// change has come for 300 ms, and holds the last count.
#[gpui::test]
fn ten_changes_ten_ms_apart_write_the_file_once(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, views) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let id = window_id(window, &mut app);

        for count in 1..=10u64 {
            change(&views, &mut app, id, "a", count);
            if count < 10 {
                app.executor().advance_clock(Duration::from_millis(10));
            }
        }
        assert!(
            !file.exists(),
            "a burst of ten changes wrote before its last change"
        );

        app.executor().advance_clock(Duration::from_millis(299));
        app.run_until_parked();
        assert!(
            !file.exists(),
            "the save did not wait until no change had come for 300 ms"
        );

        app.executor().advance_clock(Duration::from_millis(10));
        app.run_until_parked();
        assert_eq!(saved_count(&file, "a"), 10);
        assert_eq!(
            trace.state_saves(),
            1,
            "a burst of ten changes wrote the file more than once"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A steady stream of changes still saves: about every 2 s, never more
// often, and the wait after the stream holds the last count.
#[gpui::test]
fn a_steady_stream_of_changes_saves_about_every_2_s(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, views) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let id = window_id(window, &mut app);

        for count in 1..=50u64 {
            change(&views, &mut app, id, "a", count);
            app.executor().advance_clock(Duration::from_millis(100));
            if count == 25 {
                assert!(
                    file.exists(),
                    "no save had come by 2.5 s into a steady stream"
                );
            }
        }
        let saves = trace.state_saves();
        assert!(
            (2..=3).contains(&saves),
            "5 s of changes every 100 ms wrote {saves} times, not about every 2 s"
        );

        settle(&mut app);
        assert_eq!(saved_count(&file, "a"), 50);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// The file written after the wait holds the latest states, and a new run
// from it gives them back — without the first run having quit.
#[gpui::test]
fn the_wait_writes_states_a_new_run_gives_back(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, views) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let id = window_id(window, &mut app);
        change(&views, &mut app, id, "a", 7);
        change(&views, &mut app, id, "b", 4);
        settle(&mut app);
        assert_eq!(saved_count(&file, "a"), 7);
        assert_eq!(saved_count(&file, "b"), 4);

        let mut restart = cx.new_app();
        let (_host, views) = extension(&mut restart);
        restart.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut restart);
        let id = window_id(window, &mut restart);
        assert_eq!(count(&views, &mut restart, id, "a"), 7);
        assert_eq!(count(&views, &mut restart, id, "b"), 4);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A change while a save is being written leads to one more save after it,
// holding the newer state. The scheduler's steps run one at a time, and the
// clock moves to each timer as they run dry, so the change lands while the
// save's group is open and its write is still queued on the scheduler.
#[gpui::test]
fn a_change_while_a_save_is_written_leads_to_one_more_save(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, views) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let id = window_id(window, &mut app);

        change(&views, &mut app, id, "a", 1);
        app.run_until_parked();
        let mut ticks = 0;
        while trace.state_saves() == 0 && ticks < 50 {
            if !app.executor().tick()
                && !app
                    .executor()
                    .dispatcher()
                    .as_test()
                    .unwrap()
                    .advance_clock_to_next_timer()
            {
                break;
            }
            ticks += 1;
        }
        assert!(trace.state_saves() >= 1, "the save never started");
        // The write is on the scheduler, not yet run: the save is being
        // written.
        change(&views, &mut app, id, "a", 2);
        settle(&mut app);

        assert_eq!(
            trace.state_saves(),
            2,
            "a change while the save was being written led to no more save"
        );
        assert_eq!(
            saved_count(&file, "a"),
            2,
            "the newer state did not reach the file"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// Quitting with a save waiting writes the file once: the wait is cancelled
// and the quit's own write holds the states as they were at the quit.
#[gpui::test]
fn quitting_with_a_save_waiting_writes_the_file_once(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("state.json");
        let mut app = cx.new_app();
        let (_host, views) = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let window = open_and_draw(&mut app);
        let id = window_id(window, &mut app);
        change(&views, &mut app, id, "a", 5);

        app.quit();
        app.run_until_parked();

        assert_eq!(saved_count(&file, "a"), 5);
        assert_eq!(
            trace.state_saves(),
            1,
            "quitting with a save waiting wrote the file more than once"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
