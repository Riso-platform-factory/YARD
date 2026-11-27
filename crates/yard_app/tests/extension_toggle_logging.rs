//! Logging and lifecycle trace acceptance tests for roadmap #14
//! `extension-toggle` (contract points 1, 2, 7, and 9).
//!
//! Every test in this binary shares one lock, one process-wide diagnostic
//! logger, and its own `tracing::subscriber::with_default` subscriber, so no
//! test runs without a subscriber or observes another test's log records.

use gpui::{
    App, AppContext, Global, IntoElement, Menu, MenuItem, ParentElement, Styled, TestAppContext,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_app::{Application, Init, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;
use yard_shell::{Extension, Quit, Shell, Slots};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-app-extension-toggle-logging-{}-{}-{nanos:x}",
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

static LOCK: Mutex<()> = Mutex::new(());
static DIAGNOSTIC_INIT: Once = Once::new();
static DIAGNOSTICS: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push((
                    record.level(),
                    record.target().to_owned(),
                    record.args().to_string(),
                ));
        }
    }

    fn flush(&self) {}
}

fn diagnostics() -> Vec<(log::Level, String, String)> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

fn diagnostic_errors() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _, _)| *level == log::Level::Error)
        .map(|(_, target, message)| format!("[{target}] {message}"))
        .collect()
}

fn diagnostic_warnings() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _, _)| *level == log::Level::Warn)
        .map(|(_, target, message)| format!("[{target}] {message}"))
        .collect()
}

#[derive(Clone, Debug)]
struct Group {
    id: u64,
    parent: Option<u64>,
    target: &'static str,
    level: tracing::Level,
    name: &'static str,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct Event {
    group: Option<u64>,
    target: &'static str,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct Captured {
    next_id: u64,
    open: Vec<u64>,
    entered: BTreeSet<u64>,
    groups: Vec<Group>,
    events: Vec<Event>,
}

#[derive(Default)]
struct Fields {
    message: Option<String>,
    values: BTreeMap<String, String>,
}

impl Fields {
    fn insert(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.values.insert(field.name().to_owned(), value);
        }
    }
}

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.insert(field, value.to_owned());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.insert(field, value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.insert(field, value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.insert(field, value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let debug = format!("{value:?}");
        let value = debug
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(debug.as_str());
        self.insert(field, value.to_owned());
    }
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        captured.next_id += 1;
        let id = captured.next_id;
        let parent = captured.open.last().copied();
        captured.groups.push(Group {
            id,
            parent,
            target: attributes.metadata().target(),
            level: *attributes.metadata().level(),
            name: attributes.metadata().name(),
            fields: fields.values,
        });
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(group) = captured
            .groups
            .iter_mut()
            .find(|group| group.id == id.into_u64())
        {
            group.fields.extend(fields.values);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let group = captured.open.last().copied();
        captured.events.push(Event {
            group,
            target: event.metadata().target(),
            level: *event.metadata().level(),
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        captured.open.push(id);
        captured.entered.insert(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        let mut captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        assert_eq!(captured.open.pop(), Some(id.into_u64()));
    }
}

impl Trace {
    fn cursor(&self) -> (usize, usize) {
        let captured = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        (captured.groups.len(), captured.events.len())
    }

    fn groups_since(&self, cursor: usize) -> Vec<Group> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .groups[cursor..]
            .to_vec()
    }

    fn events_since(&self, cursor: usize) -> Vec<Event> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .events[cursor..]
            .to_vec()
    }

    fn was_entered(&self, id: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .entered
            .contains(&id)
    }

    fn trace_errors_since(&self, cursor: usize) -> Vec<Event> {
        self.events_since(cursor)
            .into_iter()
            .filter(|event| {
                event.target.starts_with("yard") && event.level == tracing::Level::ERROR
            })
            .collect()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    DIAGNOSTIC_INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let _guard = LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
    assert!(
        trace
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .open
            .is_empty(),
        "every opened group was exited"
    );
}

fn test_app() -> Application {
    Application {
        product: Product {
            name: "Toggle Test",
            version: "0.1.0",
            identifier: "test.yard.toggle",
        },
        url_scheme: "toggletest",
        update_url: "",
        window_icon: None,
    }
}

fn test_menus(product: &Product) -> Vec<Menu> {
    vec![Menu::new(product.name).items(vec![MenuItem::action(
        format!("Quit {}", product.name),
        Quit,
    )])]
}

struct CardView {
    slots: Slots,
}

impl gpui::Render for CardView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().children(self.slots.views("body"))
    }
}

struct NoteView;

impl gpui::Render for NoteView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

#[derive(Default)]
struct Handles {
    e_inits: usize,
    f_inits: usize,
    f: Option<Extension>,
}

impl Global for Handles {}

fn init_f(cx: &mut App, ext: &Extension) {
    let handles = cx.default_global::<Handles>();
    handles.f_inits += 1;
    handles.f = Some(ext.clone());
    ext.register(cx, "f.note", &[], |_, _, _, cx| cx.new(|_| NoteView).into());
    ext.place_root(cx, "root", "yard.row");
    ext.place(cx, "note", "f.note", "root", "children");
    ext.place(cx, "extra", "f.note", "root", "children");
}

fn init_e(cx: &mut App, ext: &Extension) {
    cx.default_global::<Handles>().e_inits += 1;
    ext.register(cx, "e.card", &["body"], |_, slots, _, cx| {
        cx.new(|_| CardView { slots }).into()
    });
    ext.place(cx, "card", "e.card", "root", "children");
    ext.place(cx, "side", "e.card", "root", "children");
    ext.named_layout(cx, "e.focus", |layout| {
        layout.move_node("card", "root", "children", 0);
    });
}

fn extensions() -> &'static [(&'static str, Init)] {
    &[("f", init_f), ("e", init_e)]
}

fn write_settings(paths: &Paths, value: &Value) {
    let file = paths.settings_file();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn start_run(app: &mut TestAppContext, paths: &Paths) -> gpui::WindowHandle<Shell> {
    let product_app = test_app();
    let open_urls = RefCell::new(OpenUrls::default());
    let window = app.update(|cx| {
        yard_app::init(cx, product_app, paths, extensions(), test_menus, &open_urls);
        let session = yard_shell::saved_session(cx, &paths.session_file());
        yard_shell::track_session(cx, paths.session_file());
        match session {
            Some(windows) => {
                let saved = &windows[0];
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: saved.bounds,
                        display_id: saved.display,
                        ..Default::default()
                    },
                    |window, cx| yard_shell::new_session_shell(window, cx, saved),
                )
                .unwrap()
            }
            None => cx
                .open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
                .unwrap(),
        }
    });
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
    window
}

fn f_handle(app: &TestAppContext) -> Extension {
    app.read(|cx| cx.global::<Handles>().f.clone().expect("f initialized"))
}

fn init_counts(app: &TestAppContext) -> (usize, usize) {
    app.read(|cx| {
        let handles = cx.global::<Handles>();
        (handles.f_inits, handles.e_inits)
    })
}

// Contract point 1:
// An extension turned off (`"extensions": {"e": false}`): its `init` does not
// run, `e.card` is not registered, its `extension-init{extension = "e"}` group
// logs `info` `turned off`, `extensions-load` has `off = 1`, and no `ERROR` is
// logged.
#[gpui::test]
fn turned_off_extension_skips_init_and_logs_turned_off_without_error(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let paths = Paths::under(temp.path());
        write_settings(&paths, &json!({ "extensions": { "e": false } }));

        let mut app = cx.new_app();
        let window = start_run(&mut app, &paths);

        assert_eq!(
            init_counts(&app),
            (1, 0),
            "`f` runs once and turned-off `e` does not run"
        );
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert!(nodes.iter().any(|node| node.component.as_ref() == "f.note"));
            assert!(
                !nodes.iter().any(|node| node.component.as_ref() == "e.card"),
                "`e.card` must not be registered or placed: {nodes:?}"
            );
        });
        // `e.focus` was not registered either.
        let f = f_handle(&app);
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                f.show_layout(window, cx, "e.focus");
                assert_eq!(
                    yard_shell::shown_layout(cx, window).as_ref(),
                    "yard.default"
                );
            })
            .unwrap();

        let groups = trace.groups_since(0);
        let events = trace.events_since(0);

        let load_group = groups
            .iter()
            .find(|group| group.name == "extensions-load" && group.target.starts_with("yard_app"))
            .unwrap_or_else(|| panic!("missing `extensions-load` group: {groups:#?}"));
        assert_eq!(load_group.level, tracing::Level::INFO);
        assert!(trace.was_entered(load_group.id));
        assert_eq!(
            load_group.fields.get("count").map(String::as_str),
            Some("2"),
            "`extensions-load` count field: {load_group:#?}"
        );
        assert_eq!(
            load_group.fields.get("off").map(String::as_str),
            Some("1"),
            "`extensions-load` must carry `off = 1`: {load_group:#?}"
        );

        let e_group = groups
            .iter()
            .find(|group| {
                group.name == "extension-init"
                    && group.target.starts_with("yard_app")
                    && group.fields.get("extension").map(String::as_str) == Some("e")
            })
            .unwrap_or_else(|| panic!("missing `extension-init` group for `e`: {groups:#?}"));
        assert_eq!(e_group.level, tracing::Level::INFO);
        assert_eq!(e_group.parent, Some(load_group.id));
        assert!(trace.was_entered(e_group.id));

        assert!(
            events.iter().any(|event| {
                event.group == Some(e_group.id)
                    && event.level == tracing::Level::INFO
                    && event.message.as_deref() == Some("turned off")
            }),
            "expected INFO `turned off` inside `extension-init{{extension = \"e\"}}`: {events:#?}"
        );

        let trace_errors = trace.trace_errors_since(0);
        assert!(
            trace_errors.is_empty(),
            "unexpected tracing ERRORs: {trace_errors:#?}"
        );
        let errors = diagnostic_errors();
        assert!(
            errors.is_empty(),
            "unexpected diagnostic ERRORs: {errors:#?}"
        );

        app.quit();
    });
}

// Contract point 2:
// `true`, and no entry, leave an extension on. An unknown name (`"nope": false`)
// gives exactly one `WARN` in the diagnostic log and changes nothing else.
#[gpui::test]
fn true_and_omitted_keep_extensions_on_and_unknown_name_warns_once(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let paths = Paths::under(temp.path());
        write_settings(
            &paths,
            &json!({ "extensions": { "e": true, "nope": false } }),
        );

        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);

        assert_eq!(
            init_counts(&app),
            (1, 1),
            "`true` and omitted entries both leave extensions on"
        );
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert!(nodes.iter().any(|node| node.component.as_ref() == "f.note"));
            assert!(nodes.iter().any(|node| node.component.as_ref() == "e.card"));
        });

        let warnings = diagnostic_warnings();
        assert_eq!(
            warnings.len(),
            1,
            "unknown extension name must log exactly one WARN: {warnings:#?}"
        );
        assert!(
            warnings[0].contains("nope"),
            "WARN must name the unknown extension `nope`: {}",
            warnings[0]
        );

        let groups = trace.groups_since(0);
        let load_group = groups
            .iter()
            .find(|group| group.name == "extensions-load" && group.target.starts_with("yard_app"))
            .unwrap_or_else(|| panic!("missing `extensions-load` group: {groups:#?}"));
        assert_eq!(
            load_group.fields.get("off").map(String::as_str),
            Some("0"),
            "`extensions-load` must carry `off = 0`: {load_group:#?}"
        );

        let errors = diagnostic_errors();
        assert!(
            errors.is_empty(),
            "unexpected diagnostic ERRORs: {errors:#?}"
        );
        let trace_errors = trace.trace_errors_since(0);
        assert!(
            trace_errors.is_empty(),
            "unexpected tracing ERRORs: {trace_errors:#?}"
        );

        app.quit();
    });
}

// Contract point 7:
// Changing the `extensions` section while the application runs logs `info`
// `applies at the next start` in an `extensions-setting-changed` group (with
// the names that would change), and changes nothing at runtime.
#[gpui::test]
fn changing_extensions_setting_while_running_logs_next_start_and_changes_nothing(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let paths = Paths::under(temp.path());
        write_settings(&paths, &json!({ "extensions": { "e": false } }));

        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        let counts_before = init_counts(&app);

        let (group_cursor, event_cursor) = trace.cursor();

        // Flip both `e` (off -> on) and `f` (on -> off) while running.
        write_settings(&paths, &json!({ "extensions": { "e": true, "f": false } }));
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();

        let groups = trace.groups_since(group_cursor);
        let events = trace.events_since(event_cursor);

        assert!(
            groups.iter().any(|group| group.name == "settings-reload"
                && group.target.starts_with("yard_settings")),
            "settings watcher did not reload the updated file: {groups:#?}"
        );

        let changed_group = groups
            .iter()
            .find(|group| {
                group.name == "extensions-setting-changed" && group.target.starts_with("yard_app")
            })
            .unwrap_or_else(|| {
                panic!("missing `extensions-setting-changed` group after reload: {groups:#?}")
            });
        assert_eq!(changed_group.level, tracing::Level::INFO);
        assert!(trace.was_entered(changed_group.id));

        let next_start_event = events
            .iter()
            .find(|event| {
                event.group == Some(changed_group.id)
                    && event.level == tracing::Level::INFO
                    && event.message.as_deref() == Some("applies at the next start")
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing INFO `applies at the next start` inside `extensions-setting-changed`: {events:#?}"
                )
            });

        let all_field_values: String = changed_group
            .fields
            .values()
            .chain(next_start_event.fields.values())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            all_field_values.contains('e') && all_field_values.contains('f'),
            "expected changed names `e` and `f` in group/event fields; group={changed_group:#?}, event={next_start_event:#?}"
        );

        // Nothing changed in the running app: `f` stayed on and `e` stayed off.
        assert_eq!(counts_before, (1, 0), "`f` starts on and `e` starts off");
        assert_eq!(
            init_counts(&app),
            (1, 0),
            "changing `extensions` at runtime must not load `e` or reload `f`"
        );
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert!(
                nodes.iter().any(|node| node.component.as_ref() == "f.note"),
                "`f` stays loaded while running"
            );
            assert!(
                !nodes.iter().any(|node| node.component.as_ref() == "e.card"),
                "`e` stays unloaded while running"
            );
        });

        let trace_errors = trace.trace_errors_since(event_cursor);
        assert!(
            trace_errors.is_empty(),
            "unexpected tracing ERRORs: {trace_errors:#?}"
        );
        let errors = diagnostic_errors();
        assert!(
            errors.is_empty(),
            "unexpected diagnostic ERRORs: {errors:#?}"
        );

        app.quit();
    });
}

// Contract point 9:
// The shell's restore line `restored` carries `kept` with the number of kept
// operations.
#[gpui::test]
fn restored_trace_line_records_kept_operations(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let paths = Paths::under(temp.path());

        // Run 1: two edits on `e`'s nodes and one edit on `f`'s node.
        {
            let mut app = cx.new_app();
            let _window = start_run(&mut app, &paths);
            let f = f_handle(&app);
            app.update(|cx| {
                f.move_node(cx, "note", "card", "body", 0);
                f.set_hidden(cx, "side", true);
                f.set_hidden(cx, "extra", true);
            });
            app.quit();
        }

        // Run 2: `e` is off -> 1 applied (`hide extra`), 2 skipped, 2 kept.
        write_settings(&paths, &json!({ "extensions": { "e": false } }));
        let (group_cursor, event_cursor) = trace.cursor();
        {
            let mut app = cx.new_app();
            let _window = start_run(&mut app, &paths);

            let groups = trace.groups_since(group_cursor);
            let events = trace.events_since(event_cursor);
            let restore_group_ids: BTreeSet<u64> = groups
                .iter()
                .filter(|group| {
                    group.name == "layout-restore" && group.target.starts_with("yard_shell")
                })
                .map(|group| group.id)
                .collect();

            let restored = events
                .iter()
                .find(|event| {
                    event.target.starts_with("yard_shell")
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("restored")
                        && event
                            .group
                            .is_some_and(|group| restore_group_ids.contains(&group))
                })
                .unwrap_or_else(|| {
                    panic!("missing `restored` event in `layout-restore`: {events:#?}")
                });

            assert_eq!(
                restored.fields.get("kept").map(String::as_str),
                Some("2"),
                "restored event must carry `kept = 2`: {restored:#?}"
            );
            assert_eq!(
                restored.fields.get("applied").map(String::as_str),
                Some("1"),
                "restored event: {restored:#?}"
            );
            assert_eq!(
                restored.fields.get("skipped").map(String::as_str),
                Some("2"),
                "restored event: {restored:#?}"
            );
            assert_eq!(init_counts(&app), (1, 0));

            app.quit();
        }

        // Run 3: `e` is on again -> 3 applied, 0 skipped, 0 kept.
        write_settings(&paths, &json!({}));
        let (group_cursor, event_cursor) = trace.cursor();
        {
            let mut app = cx.new_app();
            let _window = start_run(&mut app, &paths);
            assert_eq!(init_counts(&app), (1, 1));

            let groups = trace.groups_since(group_cursor);
            let events = trace.events_since(event_cursor);
            let restore_group_ids: BTreeSet<u64> = groups
                .iter()
                .filter(|group| {
                    group.name == "layout-restore" && group.target.starts_with("yard_shell")
                })
                .map(|group| group.id)
                .collect();

            let restored = events
                .iter()
                .find(|event| {
                    event.target.starts_with("yard_shell")
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("restored")
                        && event
                            .group
                            .is_some_and(|group| restore_group_ids.contains(&group))
                })
                .unwrap_or_else(|| panic!("missing `restored` event in run 3: {events:#?}"));

            assert_eq!(
                restored.fields.get("applied").map(String::as_str),
                Some("3"),
                "restored event in run 3: {restored:#?}"
            );
            assert_eq!(
                restored.fields.get("skipped").map(String::as_str),
                Some("0"),
                "restored event in run 3: {restored:#?}"
            );
            assert_eq!(
                restored.fields.get("kept").map(String::as_str),
                Some("0"),
                "restored event in run 3 must carry `kept = 0`: {restored:#?}"
            );

            app.quit();
        }
    });
}
