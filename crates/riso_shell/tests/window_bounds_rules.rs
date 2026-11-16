use gpui::{AppContext, TestAppContext, px};
use riso_shell::{saved_window_bounds, track_window_bounds};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
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
            "riso-shell-window-bounds-rules-{}-{}-{nanos:x}",
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
// under one lock and its own lifecycle subscriber, so a test never sees
// another test's records, and no thread reaches the shell's log lines without
// a subscriber.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell") || metadata.target().starts_with("riso_paths")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap()
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
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    RECORDS.lock().unwrap().clear();
    guard
}

fn at_level(level: log::Level) -> Vec<String> {
    RECORDS
        .lock()
        .unwrap()
        .iter()
        .filter(|(at, _)| *at == level)
        .map(|(_, message)| message.clone())
        .collect()
}

// The shell's lifecycle events, `<LEVEL> <fields>`, trace level included.
#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

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
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let line = format!("{}{}", event.metadata().level(), fields.0);
        self.0.lock().unwrap().push(line);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

impl Trace {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
}

fn last_width(file: &Path) -> serde_json::Value {
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    json["bounds"]["width"].clone()
}

fn display_id_and_uuid(app: &TestAppContext) -> (gpui::DisplayId, String) {
    app.update(|cx| {
        let display = cx.displays().into_iter().next().unwrap();
        (display.id(), display.uuid().unwrap().to_string())
    })
}

// Opens a window at `bounds`, tracks it into `file`, activates it, and lets
// whatever the activation queued finish, so that the next change starts a
// fresh save.
fn tracked_active_window(
    app: &mut TestAppContext,
    bounds: gpui::Bounds<gpui::Pixels>,
    file: &Path,
) -> gpui::AnyWindowHandle {
    let window = app.update(|cx| {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| gpui::Empty),
        )
        .unwrap()
    });
    let window = gpui::AnyWindowHandle::from(window);
    let track_file = file.to_path_buf();
    window
        .update(app, move |_, window, cx| {
            track_window_bounds(window, cx, track_file)
        })
        .unwrap();
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(150));
    window
}

// A save that cannot write is a WARN, and the next change tries again.
#[gpui::test]
fn a_failed_write_warns_and_the_next_change_tries_again(cx: &mut TestAppContext) {
    captured(cx, |cx, _trace| {
        let temp = TempDir::new();
        // The file's folder is a file, so the write cannot create it.
        let blocker = temp.path().join("data");
        fs::write(&blocker, "not a folder").unwrap();
        let file = blocker.join("window.json");
        let mut app = cx.new_app();
        let bounds = gpui::Bounds::new(
            gpui::point(px(100.), px(60.)),
            gpui::size(px(800.), px(600.)),
        );
        let window = tracked_active_window(&mut app, bounds, &file);
        RECORDS.lock().unwrap().clear();

        app.simulate_window_resize(window, gpui::size(px(900.), px(700.)));
        app.executor().advance_clock(Duration::from_millis(100));
        assert_eq!(
            at_level(log::Level::Warn).len(),
            1,
            "{:?}",
            at_level(log::Level::Warn)
        );
        assert!(!file.exists());

        fs::remove_file(&blocker).unwrap();
        app.simulate_window_resize(window, gpui::size(px(950.), px(720.)));
        app.executor().advance_clock(Duration::from_millis(100));
        assert!(
            file.exists(),
            "the change after a failed write did not save"
        );
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["bounds"]["width"], 950.);
        assert_eq!(at_level(log::Level::Warn).len(), 1);
        assert!(at_level(log::Level::Error).is_empty());
    });
}

// A screen left of and above the main one gives negative coordinates; they
// are saved and read back as they are.
#[gpui::test]
fn negative_coordinates_are_saved_and_read_back(cx: &mut TestAppContext) {
    captured(cx, |cx, _trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let (display_id, display_uuid) = display_id_and_uuid(&app);
        let bounds = gpui::Bounds::new(
            gpui::point(px(-1440.), px(-200.)),
            gpui::size(px(800.), px(600.)),
        );
        let window = tracked_active_window(&mut app, bounds, &file);
        if file.exists() {
            fs::remove_file(&file).unwrap();
        }

        app.simulate_window_resize(window, gpui::size(px(820.), px(610.)));
        app.executor().advance_clock(Duration::from_millis(100));
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(json["display"], display_uuid);
        assert_eq!(json["bounds"]["x"], -1440.);
        assert_eq!(json["bounds"]["y"], -200.);

        let saved = app.update(|cx| saved_window_bounds(cx, &file));
        assert_eq!(
            saved,
            Some((
                gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    gpui::point(px(-1440.), px(-200.)),
                    gpui::size(px(820.), px(610.)),
                )),
                display_id
            ))
        );
        assert!(at_level(log::Level::Warn).is_empty());
        assert!(at_level(log::Level::Error).is_empty());
    });
}

// Fields the format does not name are ignored, at the top and in `bounds`.
#[gpui::test]
fn a_file_with_extra_fields_is_read(cx: &mut TestAppContext) {
    captured(cx, |cx, _trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let app = cx.new_app();
        let (display_id, display_uuid) = display_id_and_uuid(&app);
        fs::write(
            &file,
            json!({
                "version": 1,
                "display": display_uuid,
                "note": "written by hand",
                "bounds": {
                    "state": "maximized",
                    "x": 10.,
                    "y": 20.,
                    "width": 640.,
                    "height": 480.,
                    "scale": 2,
                },
            })
            .to_string(),
        )
        .unwrap();

        let saved = app.update(|cx| saved_window_bounds(cx, &file));
        assert_eq!(
            saved,
            Some((
                gpui::WindowBounds::Maximized(gpui::Bounds::new(
                    gpui::point(px(10.), px(20.)),
                    gpui::size(px(640.), px(480.)),
                )),
                display_id
            ))
        );
        assert!(at_level(log::Level::Warn).is_empty());
        assert!(at_level(log::Level::Error).is_empty());
    });
}

// The last bounds of a burst of changes are written, and a change after the
// write queues another save.
#[gpui::test]
fn the_last_bounds_are_written_and_a_later_change_saves_again(cx: &mut TestAppContext) {
    captured(cx, |cx, _trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let bounds = gpui::Bounds::new(
            gpui::point(px(100.), px(60.)),
            gpui::size(px(800.), px(600.)),
        );
        let window = tracked_active_window(&mut app, bounds, &file);
        if file.exists() {
            fs::remove_file(&file).unwrap();
        }

        for width in [810., 820., 830., 840.] {
            app.simulate_window_resize(window, gpui::size(px(width), px(600.)));
            app.executor().advance_clock(Duration::from_millis(20));
        }
        app.executor().advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        assert_eq!(last_width(&file), 840.);

        app.simulate_window_resize(window, gpui::size(px(870.), px(600.)));
        app.executor().advance_clock(Duration::from_millis(100));
        app.run_until_parked();
        assert_eq!(
            last_width(&file),
            870.,
            "a change after the write did not save"
        );
        assert!(at_level(log::Level::Warn).is_empty());
        assert!(at_level(log::Level::Error).is_empty());
    });
}

// A change while the window is not active is a trace line with
// `active=false`; a change while a save waits is one with `active=true`.
#[gpui::test]
fn bounds_changes_that_start_no_save_are_trace_lines(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| gpui::Empty)
            })
            .unwrap()
        });
        let window = gpui::AnyWindowHandle::from(window);
        let track_file = file.clone();
        window
            .update(&mut app, move |_, window, cx| {
                track_window_bounds(window, cx, track_file)
            })
            .unwrap();
        app.simulate_window_resize(window, gpui::size(px(700.), px(500.)));
        assert!(
            trace
                .lines()
                .contains(&"TRACE message=window bounds changed active=false".to_string()),
            "{:#?}",
            trace.lines()
        );

        window
            .update(&mut app, |_, window, _| window.activate_window())
            .unwrap();
        app.run_until_parked();
        app.simulate_window_resize(window, gpui::size(px(710.), px(510.)));
        app.simulate_window_resize(window, gpui::size(px(720.), px(520.)));
        assert!(
            trace
                .lines()
                .contains(&"TRACE message=window bounds changed active=true".to_string()),
            "{:#?}",
            trace.lines()
        );
        app.executor().advance_clock(Duration::from_millis(150));
    });
}

// A window closed while its save waits writes the last bounds before it
// goes, without waiting for the 100 ms.
#[gpui::test]
fn closing_with_a_save_waiting_writes_the_last_bounds(cx: &mut TestAppContext) {
    captured(cx, |cx, _trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let bounds = gpui::Bounds::new(
            gpui::point(px(100.), px(60.)),
            gpui::size(px(800.), px(600.)),
        );
        let window = tracked_active_window(&mut app, bounds, &file);
        if file.exists() {
            fs::remove_file(&file).unwrap();
        }

        app.simulate_window_resize(window, gpui::size(px(900.), px(700.)));
        window
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        // Run what is ready without moving the clock, so the 100 ms wait
        // never ends and only the close can write.
        while app.executor().tick() {}
        assert!(
            file.exists(),
            "closing with a save waiting did not write it"
        );
        assert_eq!(last_width(&file), 900.);
        assert!(at_level(log::Level::Warn).is_empty());
        assert!(at_level(log::Level::Error).is_empty());
    });
}

// The application quits while a save waits: the save is written before the
// quit returns, and only once, though the window then closes too.
#[gpui::test]
fn quitting_with_a_save_waiting_writes_the_last_bounds_once(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let bounds = gpui::Bounds::new(
            gpui::point(px(100.), px(60.)),
            gpui::size(px(800.), px(600.)),
        );
        let window = tracked_active_window(&mut app, bounds, &file);
        if file.exists() {
            fs::remove_file(&file).unwrap();
        }
        let saved_before = trace
            .lines()
            .iter()
            .filter(|line| line.contains("message=saved"))
            .count();

        app.simulate_window_resize(window, gpui::size(px(910.), px(710.)));
        app.quit();
        assert!(
            file.exists(),
            "quitting with a save waiting did not write it"
        );
        assert_eq!(last_width(&file), 910.);

        while app.executor().tick() {}
        let saved = trace
            .lines()
            .iter()
            .filter(|line| line.contains("message=saved"))
            .count();
        assert_eq!(saved - saved_before, 1, "{:#?}", trace.lines());
        assert!(at_level(log::Level::Warn).is_empty());
        assert!(at_level(log::Level::Error).is_empty());
    });
}
