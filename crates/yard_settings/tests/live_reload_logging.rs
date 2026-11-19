use gpui::TestAppContext;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_settings::{Settings, get, init, load, observe, register, watch};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-live-reload-{}-{}-{nanos:x}",
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
    group: Option<u64>,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Diagnostic {
    level: log::Level,
    group: Option<u64>,
    message: String,
}

struct Captured {
    next_id: u64,
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
            spans: BTreeMap::new(),
            entered: Vec::new(),
            entered_groups: BTreeSet::new(),
            groups: Vec::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn current_settings_group(&self) -> Option<u64> {
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| self.spans[id].starts_with("yard_settings"))
    }
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let group = captured.current_settings_group();
        captured.diagnostics.push(Diagnostic {
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
        let target = metadata.target();
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        if target.starts_with("yard_settings") {
            let parent = captured.current_settings_group();
            captured.groups.push(Group {
                id,
                parent,
                level: *metadata.level(),
                name: metadata.name(),
                fields: fields.values,
            });
        }
        captured.spans.insert(id, target);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("yard_settings") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let group = captured.current_settings_group();
        captured.events.push(Event {
            group,
            level: *metadata.level(),
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = CAPTURED.lock().unwrap();
        if captured.spans[&id].starts_with("yard_settings") {
            captured.entered_groups.insert(id);
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(CAPTURED.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct First {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for First {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for First {
    const KEY: &'static str = "first";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Second {
    label: String,
    limit: usize,
}

impl Default for Second {
    fn default() -> Self {
        Self {
            label: "panel".into(),
            limit: 3,
        }
    }
}

impl Settings for Second {
    const KEY: &'static str = "second";
}

fn assert_init_group(group: &Group, entered: &BTreeSet<u64>) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "settings-init");
    assert!(
        group.fields.is_empty(),
        "unexpected settings-init fields: {:#?}",
        group.fields
    );
}

fn assert_load_group(group: &Group, entered: &BTreeSet<u64>, expected_path: &Path) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "settings-load");
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected settings-load fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected settings-load path {display} (or {debug}), got {actual}"
    );
}

fn assert_register_group(group: &Group, entered: &BTreeSet<u64>, expected_section: &str) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "settings-register");
    assert_eq!(
        group.fields,
        BTreeMap::from([("section", expected_section.to_owned())])
    );
}

fn assert_watch_group(group: &Group, entered: &BTreeSet<u64>) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "settings-watch");
    assert_eq!(
        group.parent, None,
        "settings-watch must not belong to another settings group"
    );
    assert!(
        group.fields.is_empty(),
        "unexpected settings-watch fields: {:#?}",
        group.fields
    );
}

fn assert_reload_group(
    group: &Group,
    entered: &BTreeSet<u64>,
    expected_path: &Path,
    watch_group: u64,
) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "settings-reload");
    assert_eq!(
        group.parent,
        Some(watch_group),
        "settings-reload must belong to settings-watch"
    );
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected settings-reload fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected settings-reload path {display} (or {debug}), got {actual}"
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
        .map(|(k, v)| (*k, (*v).to_owned()))
        .collect();
    assert_eq!(event.fields, expected);
}

fn groups_since(captured: &Captured, cursor: usize, expected: usize) -> &[Group] {
    let groups = &captured.groups[cursor..];
    assert_eq!(
        groups.len(),
        expected,
        "expected {expected} groups, got {groups:#?}"
    );
    groups
}

fn events_since(captured: &Captured, cursor: usize, expected: usize) -> Vec<&Event> {
    // The contract's `trace` lines are not asserted.
    let events: Vec<&Event> = captured.events[cursor..]
        .iter()
        .filter(|event| event.level != tracing::Level::TRACE)
        .collect();
    assert_eq!(
        events.len(),
        expected,
        "expected {expected} events, got {events:#?}"
    );
    events
}

fn diagnostics_since(captured: &Captured, cursor: usize, expected: usize) -> &[Diagnostic] {
    let diagnostics = &captured.diagnostics[cursor..];
    assert_eq!(
        diagnostics.len(),
        expected,
        "expected {expected} diagnostics, got {diagnostics:#?}"
    );
    diagnostics
}

fn assert_file_warn(diagnostic: &Diagnostic, group: u64) {
    assert_eq!(diagnostic.level, log::Level::Warn);
    assert_eq!(diagnostic.group, Some(group));
    let Some(reason) = diagnostic
        .message
        .strip_prefix("the settings file cannot be read: ")
    else {
        panic!("unexpected diagnostic message: {}", diagnostic.message);
    };
    assert!(!reason.trim().is_empty());
}

fn assert_section_warn(diagnostic: &Diagnostic, group: u64, section: &str) {
    assert_eq!(diagnostic.level, log::Level::Warn);
    assert_eq!(diagnostic.group, Some(group));
    let prefix = format!("section {section} cannot be read: ");
    let Some(reason) = diagnostic.message.strip_prefix(&prefix) else {
        panic!("unexpected diagnostic message: {}", diagnostic.message);
    };
    assert!(!reason.trim().is_empty());
}

fn assert_only_reload_groups_inside_watch(captured: &Captured, watch_group: u64) {
    for group in &captured.groups {
        if group.parent == Some(watch_group) {
            assert_eq!(
                group.name, "settings-reload",
                "a check must not open a group inside settings-watch: {group:#?}"
            );
        }
    }
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn live_reload_operations_open_groups_and_log_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let path = temp.path().join("settings.json");
        fs::write(
            &path,
            r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "custom"}}"#,
        )
        .unwrap();

        let calls = Rc::new(Cell::new(0usize));
        let _subscription = cx.update(|cx| {
            init(cx);
            load(cx, path.clone());
            register::<First>(cx);
            register::<Second>(cx);
            let counted = calls.clone();
            let subscription = observe::<First>(cx, move |_| counted.set(counted.get() + 1));
            watch(cx);
            subscription
        });
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "file");
            assert_eq!(get::<Second>(cx).label, "custom");
        });

        // `watch` runs in `settings-watch` and logs `started` at DEBUG.
        let (watch_id, mut groups_cursor, mut events_cursor, mut diagnostics_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let [
                init_group,
                load_group,
                first_group,
                second_group,
                watch_group,
            ] = &captured.groups[..]
            else {
                panic!("expected 5 groups, got {:#?}", captured.groups);
            };
            assert_init_group(init_group, &captured.entered_groups);
            assert_load_group(load_group, &captured.entered_groups, &path);
            assert_register_group(first_group, &captured.entered_groups, "first");
            assert_register_group(second_group, &captured.entered_groups, "second");
            assert_watch_group(watch_group, &captured.entered_groups);

            let [
                store_created,
                loaded,
                first_registered,
                second_registered,
                started,
            ] = &captured.events[..]
            else {
                panic!("expected 5 events, got {:#?}", captured.events);
            };
            assert_debug_event(store_created, init_group.id, "store created", &[]);
            assert_debug_event(loaded, load_group.id, "loaded", &[("sections", "2")]);
            assert_debug_event(
                first_registered,
                first_group.id,
                "registered",
                &[("from", "file")],
            );
            assert_debug_event(
                second_registered,
                second_group.id,
                "registered",
                &[("from", "file")],
            );
            assert_debug_event(started, watch_group.id, "started", &[]);
            assert!(
                captured.diagnostics.is_empty(),
                "expected no diagnostics, got {:#?}",
                captured.diagnostics
            );
            (
                watch_group.id,
                captured.groups.len(),
                captured.events.len(),
                captured.diagnostics.len(),
            )
        };

        // A changed section gets its new value, its users are told, and the read
        // logs `changed {section}` and `reloaded {changed}` inside `settings-reload`.
        fs::write(
            &path,
            r#"{"first": {"greeting": "hi", "size": 20}, "second": {"label": "custom"}}"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "hi");
            assert_eq!(get::<Second>(cx).label, "custom");
        });
        assert_eq!(calls.get(), 1);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "first")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // The same content in other spaces and key order is not a change.
        fs::write(
            &path,
            r#"{"second" : { "label" : "custom" },  "first" : { "size" : 20 , "greeting" : "hi" }}"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "hi");
            assert_eq!(get::<Second>(cx).label, "custom");
        });
        assert_eq!(calls.get(), 1);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], reload.id, "reloaded", &[("changed", "0")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A file that cannot be read keeps every value and warns.
        fs::write(&path, "not json").unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "hi");
            assert_eq!(get::<Second>(cx).label, "custom");
        });
        assert_eq!(calls.get(), 1);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], reload.id, "reloaded", &[("changed", "0")]);
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_file_warn(&diagnostics[0], reload.id);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // The same broken content is not reported again over several seconds,
        // and a check opens no group of its own inside `settings-watch`.
        for _ in 0..3 {
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
        }
        {
            let captured = CAPTURED.lock().unwrap();
            groups_since(&captured, groups_cursor, 0);
            events_since(&captured, events_cursor, 0);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            assert_only_reload_groups_inside_watch(&captured, watch_id);
        }

        // A different broken content warns again.
        fs::write(&path, r#"{"second": "#).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], reload.id, "reloaded", &[("changed", "0")]);
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_file_warn(&diagnostics[0], reload.id);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A path that cannot be read warns the same way, once for the same reason.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], reload.id, "reloaded", &[("changed", "0")]);
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_file_warn(&diagnostics[0], reload.id);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        for _ in 0..2 {
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
        }
        {
            let captured = CAPTURED.lock().unwrap();
            groups_since(&captured, groups_cursor, 0);
            events_since(&captured, events_cursor, 0);
            diagnostics_since(&captured, diagnostics_cursor, 0);
        }
        fs::remove_dir(&path).unwrap();

        // The file was deleted: every section that did not have its defaults gets them.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx), &First::default());
            assert_eq!(get::<Second>(cx), &Second::default());
        });
        assert_eq!(calls.get(), 2);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 3);
            let mut sections = events[..2]
                .iter()
                .map(|event| {
                    assert_eq!(event.group, Some(reload.id));
                    assert_eq!(event.level, tracing::Level::DEBUG);
                    assert_eq!(event.message.as_deref(), Some("changed"));
                    assert_eq!(
                        event.fields.keys().copied().collect::<Vec<_>>(),
                        ["section"]
                    );
                    event.fields["section"].as_str()
                })
                .collect::<Vec<_>>();
            sections.sort();
            assert_eq!(sections, ["first", "second"]);
            assert_debug_event(events[2], reload.id, "reloaded", &[("changed", "2")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A section whose new content cannot be read keeps its value and warns;
        // another section in the same file still changes and its users are told.
        fs::write(
            &path,
            r#"{"first": {"size": "big"}, "second": {"label": "new"}}"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx), &First::default());
            assert_eq!(get::<Second>(cx).label, "new");
        });
        assert_eq!(calls.get(), 2);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "second")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_section_warn(&diagnostics[0], reload.id, "first");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // While the first section stays broken with the same content, a change
        // to the second section does not report it again.
        for label in ["newer", "new"] {
            fs::write(
                &path,
                format!(r#"{{"first": {{"size": "big"}}, "second": {{"label": "{label}"}}}}"#),
            )
            .unwrap();
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
            cx.read(|cx| {
                assert_eq!(get::<First>(cx), &First::default());
                assert_eq!(get::<Second>(cx).label, label);
            });
            assert_eq!(calls.get(), 2);
            {
                let captured = CAPTURED.lock().unwrap();
                let reload = &groups_since(&captured, groups_cursor, 1)[0];
                assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
                let events = events_since(&captured, events_cursor, 2);
                assert_debug_event(events[0], reload.id, "changed", &[("section", "second")]);
                assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
                diagnostics_since(&captured, diagnostics_cursor, 0);
                groups_cursor = captured.groups.len();
                events_cursor = captured.events.len();
                diagnostics_cursor = captured.diagnostics.len();
            }
        }

        // Across these reads, `section first cannot be read` was logged once.
        {
            let captured = CAPTURED.lock().unwrap();
            let reports = captured
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.message.starts_with("section "))
                .count();
            assert_eq!(
                reports, 1,
                "expected `section first cannot be read` exactly once, got {:#?}",
                captured.diagnostics
            );
        }

        // The broken section recovers when its content reads again.
        fs::write(
            &path,
            r#"{"first": {"greeting": "back", "size": 30}, "second": {"label": "new"}}"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            let first = get::<First>(cx);
            assert_eq!(first.greeting, "back");
            assert_eq!(first.size, 30);
            assert!(first.enabled);
            assert_eq!(get::<Second>(cx).label, "new");
        });
        assert_eq!(calls.get(), 3);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "first")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A removed key gives the section its defaults and tells its users.
        fs::write(&path, r#"{"second": {"label": "new"}}"#).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx), &First::default());
            assert_eq!(get::<Second>(cx).label, "new");
        });
        assert_eq!(calls.get(), 4);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "first")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A deleted file leaves the sections that already have their defaults alone.
        fs::remove_file(&path).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx), &First::default());
            assert_eq!(get::<Second>(cx), &Second::default());
        });
        assert_eq!(calls.get(), 4);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "second")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A second `watch` does nothing but `already watching`.
        cx.update(watch);
        {
            let captured = CAPTURED.lock().unwrap();
            let second_watch = &groups_since(&captured, groups_cursor, 1)[0];
            assert_watch_group(second_watch, &captured.entered_groups);
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], second_watch.id, "already watching", &[]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // ... and the one running check still applies an edit once.
        fs::write(
            &path,
            r#"{"first": {"greeting": "final", "size": 40}, "second": {"label": "last"}}"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "final");
            assert_eq!(get::<Second>(cx).label, "last");
        });
        assert_eq!(calls.get(), 5);
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = &groups_since(&captured, groups_cursor, 1)[0];
            assert_reload_group(reload, &captured.entered_groups, &path, watch_id);
            let events = events_since(&captured, events_cursor, 3);
            let mut sections = events[..2]
                .iter()
                .map(|event| {
                    assert_eq!(event.group, Some(reload.id));
                    assert_eq!(event.level, tracing::Level::DEBUG);
                    assert_eq!(event.message.as_deref(), Some("changed"));
                    assert_eq!(
                        event.fields.keys().copied().collect::<Vec<_>>(),
                        ["section"]
                    );
                    event.fields["section"].as_str()
                })
                .collect::<Vec<_>>();
            sections.sort();
            assert_eq!(sections, ["first", "second"]);
            assert_debug_event(events[2], reload.id, "reloaded", &[("changed", "2")]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // Without a `load` there is no file to watch.
        let app = cx.new_app();
        app.update(|cx| {
            init(cx);
            watch(cx);
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = groups_since(&captured, groups_cursor, 2);
            assert_init_group(&groups[0], &captured.entered_groups);
            assert_watch_group(&groups[1], &captured.entered_groups);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], groups[0].id, "store created", &[]);
            assert_debug_event(events[1], groups[1].id, "no settings file to watch", &[]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
}
