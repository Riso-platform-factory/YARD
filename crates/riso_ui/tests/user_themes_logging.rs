#![cfg(feature = "dev")]

// Alone in this binary: installs the process-wide diagnostic logger and
// captures every callsite.
use gpui::TestAppContext;
use riso_ui::dev::load_user_themes;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
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
            "riso-ui-user-themes-logging-{}-{}-{nanos:x}",
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

#[derive(Debug)]
struct Group {
    id: u64,
    parent: Option<u64>,
    level: tracing::Level,
    name: &'static str,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Event {
    seq: u64,
    group: Option<u64>,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Diagnostic {
    seq: u64,
    level: log::Level,
    group: Option<u64>,
    message: String,
}

struct Captured {
    next_id: u64,
    next_seq: u64,
    spans: BTreeMap<u64, &'static str>,
    entered: Vec<u64>,
    entered_groups: BTreeSet<u64>,
    groups: Vec<Group>,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

impl Captured {
    const fn new() -> Self {
        Self {
            next_id: 1,
            next_seq: 1,
            spans: BTreeMap::new(),
            entered: Vec::new(),
            entered_groups: BTreeSet::new(),
            groups: Vec::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    // The innermost entered span of the same crate: the group open at that moment.
    fn current_group(&self, target: &str) -> Option<u64> {
        let krate = crate_of(target);
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| crate_of(self.spans[id]) == krate)
    }
}

fn crate_of(target: &str) -> &str {
    target.split("::").next().unwrap_or(target)
}

fn watched(target: &str) -> bool {
    crate_of(target) == "riso_ui"
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        watched(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_group(record.metadata().target());
        captured.diagnostics.push(Diagnostic {
            seq,
            level: record.level(),
            group,
            message,
        });
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Fields {
    message: Option<String>,
    values: BTreeMap<&'static str, String>,
}

impl Fields {
    fn insert(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.values.insert(field.name(), value);
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
        self.insert(field, format!("{value:?}"));
    }
}

struct Trace;

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
        let metadata = attributes.metadata();
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        let parent = if attributes.is_contextual() {
            captured.entered.last().copied()
        } else {
            attributes.parent().map(|id| id.into_u64())
        };
        if watched(metadata.target()) {
            captured.groups.push(Group {
                id,
                parent,
                level: *metadata.level(),
                name: metadata.name(),
                fields: fields.values,
            });
        }
        captured.spans.insert(id, metadata.target());
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let id = id.into_u64();
        if let Some(group) = captured.groups.iter_mut().find(|group| group.id == id) {
            group.fields.extend(fields.values);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !watched(metadata.target()) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_group(metadata.target());
        captured.events.push(Event {
            seq,
            group,
            level: *metadata.level(),
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = CAPTURED.lock().unwrap();
        if watched(captured.spans[&id]) {
            captured.entered_groups.insert(id);
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(CAPTURED.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

// A field's text, however it was recorded (`str`, `%`, or `?`).
fn text(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

// A theme family in Zed's format, small enough for the test: a `name`, an
// `author`, and `themes`, each with a `name`, an `appearance`, and a `style`.
const FAMILY: &str = r##"{
    "name": "Logged",
    "author": "riso's tests",
    "themes": [
        {"name": "Logged One", "appearance": "dark", "style": {"background": "#101010ff"}},
        {"name": "Logged Two", "appearance": "light", "style": {"background": "#efeff0ff"}}
    ]
}"##;

// The load inside `app-init`, as the startup does it: the UI first, then the
// user's theme files. Returns app-init's id and the count of themes registered.
fn load_in_app_init(cx: &mut gpui::App, dir: &Path) -> (u64, usize) {
    let app_init = tracing::info_span!("app-init");
    let app_init_id = app_init.id().unwrap().into_u64();
    let count = {
        let _group = app_init.entered();
        riso_ui::init(cx);
        load_user_themes(cx, dir)
    };
    (app_init_id, count)
}

fn assert_group(group: &Group, entered: &BTreeSet<u64>, name: &str) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, name);
}

fn assert_themes_load_group(
    group: &Group,
    entered: &BTreeSet<u64>,
    app_init: u64,
    expected_path: &Path,
) {
    assert_group(group, entered, "themes-load");
    assert_eq!(
        group.parent,
        Some(app_init),
        "themes-load must sit inside the group open when load_user_themes is called"
    );
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected themes-load fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected themes-load path {display} (or {debug}), got {actual}"
    );
}

fn assert_debug_event(
    event: &Event,
    expected_group: u64,
    expected_message: &str,
    expected_fields: &[(&'static str, &str)],
) {
    assert_eq!(event.group, Some(expected_group));
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some(expected_message));
    let expected: BTreeMap<&'static str, String> = expected_fields
        .iter()
        .map(|(key, value)| (*key, (*value).to_owned()))
        .collect();
    let actual: BTreeMap<&'static str, String> = event
        .fields
        .iter()
        .map(|(key, value)| (*key, text(value).to_owned()))
        .collect();
    assert_eq!(actual, expected);
}

fn events_in_group(captured: &Captured, cursor: usize, group: u64) -> Vec<&Event> {
    captured.events[cursor..]
        .iter()
        .filter(|event| event.group == Some(group))
        .collect()
}

#[gpui::test]
fn load_user_themes_opens_themes_load_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        // 1. A folder whose files meet every fate, in the order of their
        //    names: a family is read, a file is not valid JSON, a file is JSON
        //    but not a theme family, a `.txt` is skipped without a word, and a
        //    file in a subfolder is never read. `themes-load{path}` wraps it
        //    all inside `app-init`, and `user themes loaded` ends it.
        let dir = temp.path().join("themes");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a-family.json"), FAMILY).unwrap();
        fs::write(
            dir.join("b-broken.json"),
            r#"{"name": "Broken", "themes": ["#,
        )
        .unwrap();
        fs::write(dir.join("c-notafamily.json"), r#"{"name": "No themes"}"#).unwrap();
        fs::write(dir.join("d-notes.txt"), FAMILY).unwrap();
        fs::write(dir.join("sub/e-inner.json"), FAMILY).unwrap();

        let (app_init, count) = cx.update(|cx| load_in_app_init(cx, &dir));
        assert_eq!(count, 2);
        let (groups_cursor, events_cursor, diagnostics_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let [ui_init, load] = &captured.groups[..] else {
                panic!(
                    "expected ui-init and themes-load, got {:#?}",
                    captured.groups
                );
            };
            assert_group(ui_init, &captured.entered_groups, "ui-init");
            assert_themes_load_group(load, &captured.entered_groups, app_init, &dir);

            let events = events_in_group(&captured, 0, load.id);
            assert_eq!(
                events.len(),
                2,
                "expected 2 events inside themes-load, got {events:#?}"
            );
            let (loaded, done) = (events[0], events[1]);
            assert_debug_event(
                loaded,
                load.id,
                "theme file loaded",
                &[("file", "a-family.json"), ("themes", "2")],
            );
            assert_debug_event(done, load.id, "user themes loaded", &[("count", "2")]);

            let [broken_warn, family_warn] = &captured.diagnostics[..] else {
                panic!("expected 2 diagnostics, got {:#?}", captured.diagnostics);
            };
            assert_eq!(broken_warn.level, log::Level::Warn);
            assert_eq!(broken_warn.group, Some(load.id));
            let Some(reason) = broken_warn
                .message
                .strip_prefix("the theme file b-broken.json cannot be read: ")
            else {
                panic!("unexpected diagnostic message: {}", broken_warn.message);
            };
            assert!(!reason.trim().is_empty());
            assert_eq!(family_warn.level, log::Level::Warn);
            assert_eq!(family_warn.group, Some(load.id));
            assert_eq!(
                family_warn.message,
                "the theme file c-notafamily.json cannot be read: not a theme family in Zed's format"
            );

            // The files are read in the order of their names.
            assert!(
                loaded.seq < broken_warn.seq
                    && broken_warn.seq < family_warn.seq
                    && family_warn.seq < done.seq,
                "the load must take the files in the order of their names"
            );
            (
                captured.groups.len(),
                captured.events.len(),
                captured.diagnostics.len(),
            )
        };

        // 2. A folder that is not there: `no themes folder` inside
        //    `themes-load{path}`, and nothing more is said.
        let missing = temp.path().join("absent");
        assert!(!missing.exists());
        let app = cx.new_app();
        let (app_init, count) = app.update(|cx| load_in_app_init(cx, &missing));
        assert_eq!(count, 0);
        {
            let captured = CAPTURED.lock().unwrap();
            let [ui_init, load] = &captured.groups[groups_cursor..] else {
                panic!(
                    "expected ui-init and themes-load, got {:#?}",
                    &captured.groups[groups_cursor..]
                );
            };
            assert_group(ui_init, &captured.entered_groups, "ui-init");
            assert_themes_load_group(load, &captured.entered_groups, app_init, &missing);

            let events = events_in_group(&captured, events_cursor, load.id);
            assert_eq!(
                events.len(),
                1,
                "expected `no themes folder` alone inside themes-load, got {events:#?}"
            );
            assert_debug_event(events[0], load.id, "no themes folder", &[]);
            assert!(
                captured.diagnostics[diagnostics_cursor..].is_empty(),
                "expected no diagnostics, got {:#?}",
                &captured.diagnostics[diagnostics_cursor..]
            );
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
}
