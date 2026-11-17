#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, TestAppContext, px};
use riso_layout::{Operation, decode};
use riso_shell::dev as shell;
use riso_shell::dev::{SessionWindow, new_session_shell, saved_session, track_session};
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
            "riso-shell-session-rules-{}-{}-{nanos:x}",
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
// another test's records, and no thread reaches the shell's log lines
// without a subscriber.
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

fn problems() -> Vec<(log::Level, String)> {
    RECORDS
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level <= log::Level::Warn)
        .cloned()
        .collect()
}

#[derive(Default)]
struct Captured {
    spans: Vec<String>,
    entered: Vec<u64>,
    lines: Vec<String>,
}

// Records each event as `<innermost group>: <LEVEL> <fields>`.
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
        let mut captured = self.0.lock().unwrap();
        captured
            .spans
            .push(attributes.metadata().name().to_string());
        let id = captured.spans.len() as u64;
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !target.starts_with("riso_shell") || *event.metadata().level() == tracing::Level::TRACE {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured.entered.last().map_or("none".to_string(), |id| {
            captured.spans[*id as usize - 1].clone()
        });
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

impl Trace {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().lines.clone()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    RECORDS.lock().unwrap().clear();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
}

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

// `tour` registers `tour.card`, places a row of cards `a`, `b`, `c`, and
// names `tour.swap`, which shows `c` first.
fn extension(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
        host.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "root", "children");
        }
        host.named_layout(cx, "tour.swap", |layout| {
            layout.move_node("c", "root", "children", 0);
        });
        host
    })
}

fn open(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    })
}

fn reopen(app: &mut TestAppContext, saved: &SessionWindow) -> gpui::WindowHandle<shell::Shell> {
    app.update(|cx| {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: saved.bounds,
                display_id: saved.display,
                ..Default::default()
            },
            |window, cx| new_session_shell(window, cx, saved),
        )
        .unwrap()
    })
}

fn activate(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
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

fn inner_bounds(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
) -> gpui::WindowBounds {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, _| window.inner_window_bounds())
        .unwrap()
}

fn settle(app: &mut TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(200));
    app.run_until_parked();
}

fn session_json(file: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
}

fn hide(node: &str) -> serde_json::Value {
    json!([{"op": "hide", "node": node}])
}

// A window whose screen is gone opens at the default place, as feature 7's
// rule says, with its layers.
#[gpui::test]
fn a_window_on_a_screen_that_is_gone_opens_at_the_default_place(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        fs::write(
            &file,
            json!({
                "version": 1,
                "windows": [{
                    "display": "00000000-0000-0000-0000-000000000000",
                    "bounds": {"state": "windowed", "x": 333., "y": 222., "width": 444., "height": 321.},
                    "layout": "riso.default",
                    "layers": {"riso.default": hide("b")},
                }],
            })
            .to_string(),
        )
        .unwrap();
        let mut app = cx.new_app();
        extension(&mut app);
        let saved = app
            .update(|cx| saved_session(cx, &file))
            .expect("a saved session");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].bounds, None);
        assert_eq!(saved[0].display, None);
        assert!(
            trace
                .lines()
                .iter()
                .any(|line| line.starts_with("session-restore: INFO")
                    && line.contains("no screen has the saved UUID")),
            "{:#?}",
            trace.lines()
        );

        let default = open(&mut app);
        let default_bounds = inner_bounds(default, &mut app);
        let window = reopen(&mut app, &saved[0]);
        assert_eq!(inner_bounds(window, &mut app), default_bounds);
        activate(window, &mut app);
        assert!(hidden(&mut app, "b"));
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// A window that showed a named layout no longer registered opens with
// `riso.default` and its layer for it, with an `INFO`.
#[gpui::test]
fn a_named_layout_no_longer_registered_opens_with_the_default(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        fs::write(
            &file,
            json!({
                "version": 1,
                "windows": [{
                    "display": null,
                    "bounds": {"state": "windowed", "x": 0., "y": 0., "width": 800., "height": 600.},
                    "layout": "tour.gone",
                    "layers": {"tour.gone": hide("a"), "riso.default": hide("c")},
                }],
            })
            .to_string(),
        )
        .unwrap();
        let mut app = cx.new_app();
        extension(&mut app);
        let saved = app
            .update(|cx| saved_session(cx, &file))
            .expect("a saved session");
        let window = reopen(&mut app, &saved[0]);
        assert_eq!(shown(window, &mut app), "riso.default");
        activate(window, &mut app);
        assert!(hidden(&mut app, "c"));
        assert!(!hidden(&mut app, "a"));
        assert!(
            trace.lines().iter().any(|line| line.contains(": INFO")
                && line.contains("no named layout")
                && line.contains("tour.gone")),
            "{:#?}",
            trace.lines()
        );
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// Three windows are saved in the order they opened and restored in it,
// each with its own layout and layer.
#[gpui::test]
fn three_windows_are_saved_and_restored_in_order(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        let mut app = cx.new_app();
        let host = extension(&mut app);
        let layouts = temp.path().join("layouts");
        app.update(|cx| {
            shell::set_named_layout_files(cx, move |name| layouts.join(format!("{name}.json")));
            track_session(cx, file.clone());
        });
        let mut windows = Vec::new();
        for node in ["a", "b", "c"] {
            let window = open(&mut app);
            activate(window, &mut app);
            app.update(|cx| host.set_hidden(cx, node, true));
            windows.push(window);
        }
        gpui::AnyWindowHandle::from(windows[1])
            .update(&mut app, |_, window, cx| {
                host.show_layout(window, cx, "tour.swap")
            })
            .unwrap();
        settle(&mut app);

        let json = session_json(&file);
        assert_eq!(json["windows"].as_array().unwrap().len(), 3, "{json}");
        let saved = app
            .update(|cx| saved_session(cx, &file))
            .expect("a saved session");
        assert_eq!(
            saved
                .iter()
                .map(|window| window.layout.as_str())
                .collect::<Vec<_>>(),
            ["riso.default", "tour.swap", "riso.default"]
        );

        let mut restart = cx.new_app();
        extension(&mut restart);
        let reopened = saved
            .iter()
            .map(|window| reopen(&mut restart, window))
            .collect::<Vec<_>>();
        for (window, (layout, node)) in reopened.into_iter().zip([
            ("riso.default", "a"),
            ("tour.swap", ""),
            ("riso.default", "c"),
        ]) {
            assert_eq!(shown(window, &mut restart), layout);
            activate(window, &mut restart);
            for other in ["a", "b", "c"] {
                assert_eq!(
                    hidden(&mut restart, other),
                    other == node,
                    "{layout} {other}"
                );
            }
        }
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// A session written while the main window's saves wait — `window.json`
// waits its 100 ms and the session too — holds what `layout.json` and
// `window.json` hold when the application quits.
#[gpui::test]
fn a_session_written_while_a_save_waits_matches_the_main_window_files(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        let layout = temp.path().join("layout.json");
        let window_file = temp.path().join("window.json");
        let mut app = cx.new_app();
        let host = extension(&mut app);
        app.update(|cx| {
            shell::set_layout_file(cx, layout.clone());
            track_session(cx, file.clone());
        });
        let main = open(&mut app);
        activate(main, &mut app);
        main.update(&mut app, |_, window, cx| {
            riso_shell::track_window_bounds(window, cx, window_file.clone())
        })
        .unwrap();
        settle(&mut app);

        app.update(|cx| host.set_hidden(cx, "b", true));
        app.simulate_window_resize(
            gpui::AnyWindowHandle::from(main),
            gpui::size(px(700.), px(500.)),
        );
        app.run_until_parked();
        // `layout.json` is written at once; the session waits.
        let ops: Vec<Operation> = decode(&fs::read_to_string(&layout).unwrap()).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(
            session_json(&file)["windows"][0]["layers"]["riso.default"],
            json!([])
        );

        app.quit();
        let session = session_json(&file);
        let saved_window = session_json(&window_file);
        assert_eq!(session["windows"][0]["bounds"], saved_window["bounds"]);
        assert_eq!(saved_window["bounds"]["width"], 700.);
        assert_eq!(session["windows"][0]["layers"]["riso.default"], hide("b"));
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// A window closed while another remains leaves the session; the last one
// stays in it, with its bounds and layers, since closing it quits.
#[gpui::test]
fn closing_the_last_window_keeps_it_in_the_session(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        let mut app = cx.new_app();
        let host = extension(&mut app);
        app.update(|cx| track_session(cx, file.clone()));
        let open_at = |app: &mut TestAppContext, x: f32| {
            app.update(|cx| {
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(x), px(90.)),
                            gpui::size(px(640.), px(480.)),
                        ))),
                        ..Default::default()
                    },
                    shell::new_shell,
                )
                .unwrap()
            })
        };
        let first = open_at(&mut app, 100.);
        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        let second = open_at(&mut app, 300.);
        activate(second, &mut app);
        app.update(|cx| host.set_hidden(cx, "b", true));
        settle(&mut app);
        assert_eq!(session_json(&file)["windows"].as_array().unwrap().len(), 2);

        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        settle(&mut app);
        let windows = session_json(&file)["windows"].clone();
        assert_eq!(windows.as_array().unwrap().len(), 1, "{windows}");
        assert_eq!(windows[0]["bounds"]["x"], 300.);

        second
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        settle(&mut app);
        let windows = session_json(&file)["windows"].clone();
        assert_eq!(windows.as_array().unwrap().len(), 1, "{windows}");
        assert_eq!(windows[0]["bounds"]["x"], 300.);
        assert_eq!(windows[0]["bounds"]["width"], 640.);
        assert_eq!(windows[0]["layout"], "riso.default");
        assert_eq!(windows[0]["layers"]["riso.default"], hide("b"));

        // The quit that follows still lists it.
        app.quit();
        let windows = session_json(&file)["windows"].clone();
        assert_eq!(windows.as_array().unwrap().len(), 1, "{windows}");
        assert_eq!(windows[0]["layers"]["riso.default"], hide("b"));
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// A window opened from the session keeps the session's layer for every
// named layout: switching to one uses it, inside `layout-restore`, even
// when the layout's file holds another layer.
#[gpui::test]
fn a_switch_uses_the_session_layer_not_the_file(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        let layouts = temp.path().join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            layouts.join("tour.swap.json"),
            json!({"version": 1, "operations": hide("a")}).to_string(),
        )
        .unwrap();
        fs::write(
            &file,
            json!({
                "version": 1,
                "windows": [{
                    "display": null,
                    "bounds": {"state": "windowed", "x": 0., "y": 0., "width": 800., "height": 600.},
                    "layout": "riso.default",
                    "layers": {"riso.default": [], "tour.swap": hide("b")},
                }],
            })
            .to_string(),
        )
        .unwrap();
        let mut app = cx.new_app();
        let host = extension(&mut app);
        app.update(|cx| {
            shell::set_layout_file(cx, temp.path().join("layout.json"));
            shell::set_named_layout_files(cx, move |name| layouts.join(format!("{name}.json")));
        });
        let saved = app
            .update(|cx| saved_session(cx, &file))
            .expect("a saved session");
        // The first window is the main window, which has files of its own.
        let window = reopen(&mut app, &saved[0]);
        activate(window, &mut app);
        let mark = trace.lines().len();
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                host.show_layout(window, cx, "tour.swap")
            })
            .unwrap();
        assert_eq!(shown(window, &mut app), "tour.swap");
        assert!(hidden(&mut app, "b"), "the session's layer did not apply");
        assert!(!hidden(&mut app, "a"), "the file's layer applied");
        let lines = trace.lines()[mark..].to_vec();
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("layout-restore: DEBUG")
                    && line.contains("applied=1")),
            "{lines:#?}"
        );
        assert!(problems().is_empty(), "{:?}", problems());
    });
}

// A window the session cannot read is left out of a save with a `WARN`,
// once in the diagnostic log however many saves leave it out.
#[gpui::test]
fn a_window_the_session_cannot_read_is_left_out_with_a_warning(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let file = temp.path().join("session.json");
        let mut app = cx.new_app();
        let host = extension(&mut app);
        app.update(|cx| track_session(cx, file.clone()));
        let first = open(&mut app);
        activate(first, &mut app);
        // The second window's shell outlives its window: the session still
        // lists it, and cannot read it.
        let kept_shell = std::rc::Rc::new(std::cell::RefCell::new(None));
        let keep = kept_shell.clone();
        let second = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), move |window, cx| {
                *keep.borrow_mut() = Some(shell::new_shell(window, cx));
                cx.new(|_| Card)
            })
            .unwrap()
        });
        second
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        settle(&mut app);
        assert_eq!(session_json(&file)["windows"].as_array().unwrap().len(), 1);
        let warnings = problems();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].1.contains("left out of the session"),
            "{warnings:?}"
        );

        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        settle(&mut app);
        assert_eq!(problems().len(), 1, "{:?}", problems());
        assert!(
            trace
                .lines()
                .iter()
                .any(|line| line.contains(": WARN") && line.contains("left out of the session")),
            "{:#?}",
            trace.lines()
        );
        assert!(kept_shell.borrow().is_some());
    });
}
