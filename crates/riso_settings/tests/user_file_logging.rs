use gpui::TestAppContext;
use riso_settings::{Settings, get, init, load, register};
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
            "riso-settings-user-file-{}-{}-{nanos:x}",
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

    fn current_settings_group(&self) -> Option<u64> {
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| self.spans[id].starts_with("riso_settings"))
    }
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_settings_group();
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
        let target = metadata.target();
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        if target.starts_with("riso_settings") {
            captured.groups.push(Group {
                id,
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
        if !metadata.target().starts_with("riso_settings") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_settings_group();
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
        if captured.spans[&id].starts_with("riso_settings") {
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

#[derive(serde::Deserialize, Default, PartialEq, Debug)]
#[serde(default)]
struct Third {
    flag: bool,
}

impl Settings for Third {
    const KEY: &'static str = "third";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Fourth {
    count: u32,
}

impl Default for Fourth {
    fn default() -> Self {
        Self { count: 5 }
    }
}

impl Settings for Fourth {
    const KEY: &'static str = "fourth";
}

#[derive(serde::Deserialize, Default, PartialEq, Debug)]
#[serde(default)]
struct FirstDuplicate {
    count: u32,
}

impl Settings for FirstDuplicate {
    const KEY: &'static str = First::KEY;
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

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn user_file_operations_open_groups_and_log_every_contract_line(cx: &mut TestAppContext) {
    assert_eq!(FirstDuplicate::default().count, 0);
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        // 1. Missing file: `no settings file` at DEBUG in `settings-load{path}`.
        let missing_path = temp.path().join("missing/settings.json");
        cx.update(|cx| {
            init(cx);
            load(cx, missing_path.clone());
            register::<First>(cx);
        });
        cx.read(|cx| {
            let first = get::<First>(cx);
            assert_eq!(first.greeting, "hello");
            assert_eq!(first.size, 14);
            assert!(first.enabled);
        });
        assert!(!missing_path.exists());

        let (mut groups_cursor, mut events_cursor, mut diagnostics_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let [init_group, load_group, reg_group] = &captured.groups[..] else {
                panic!("expected 3 groups, got {:#?}", captured.groups);
            };
            assert_init_group(init_group, &captured.entered_groups);
            assert_load_group(load_group, &captured.entered_groups, &missing_path);
            assert_register_group(reg_group, &captured.entered_groups, "first");

            let [store_created, no_file, registered] = &captured.events[..] else {
                panic!("expected 3 events, got {:#?}", captured.events);
            };
            assert_debug_event(store_created, init_group.id, "store created", &[]);
            assert_debug_event(no_file, load_group.id, "no settings file", &[]);
            assert_debug_event(
                registered,
                reg_group.id,
                "registered",
                &[("from", "defaults")],
            );
            assert!(
                captured.diagnostics.is_empty(),
                "expected no diagnostics, got {:#?}",
                captured.diagnostics
            );
            (
                captured.groups.len(),
                captured.events.len(),
                captured.diagnostics.len(),
            )
        };

        // 2. Unreadable directory, invalid JSON, and non-object JSON warn inside `settings-load`.
        let dir_path = temp.path().join("as_dir");
        fs::create_dir_all(&dir_path).unwrap();
        let invalid_json_path = temp.path().join("invalid.json");
        fs::write(&invalid_json_path, "not json").unwrap();
        let array_json_path = temp.path().join("array.json");
        fs::write(&array_json_path, "[1, 2, 3]").unwrap();

        for bad_path in [&dir_path, &invalid_json_path, &array_json_path] {
            let app = cx.new_app();
            app.update(|cx| {
                init(cx);
                load(cx, bad_path.clone());
                register::<First>(cx);
            });
            app.read(|cx| {
                assert_eq!(get::<First>(cx), &First::default());
            });

            let captured = CAPTURED.lock().unwrap();
            let [init_group, load_group, reg_group] = &captured.groups[groups_cursor..] else {
                panic!(
                    "expected 3 groups for {}, got {:#?}",
                    bad_path.display(),
                    &captured.groups[groups_cursor..]
                );
            };
            assert_init_group(init_group, &captured.entered_groups);
            assert_load_group(load_group, &captured.entered_groups, bad_path);
            assert_register_group(reg_group, &captured.entered_groups, "first");

            let [store_created, registered] = &captured.events[events_cursor..] else {
                panic!(
                    "expected 2 events for {}, got {:#?}",
                    bad_path.display(),
                    &captured.events[events_cursor..]
                );
            };
            assert_debug_event(store_created, init_group.id, "store created", &[]);
            assert_debug_event(
                registered,
                reg_group.id,
                "registered",
                &[("from", "defaults")],
            );

            let [diag] = &captured.diagnostics[diagnostics_cursor..] else {
                panic!(
                    "expected 1 diagnostic for {}, got {:#?}",
                    bad_path.display(),
                    &captured.diagnostics[diagnostics_cursor..]
                );
            };
            assert_eq!(diag.level, log::Level::Warn);
            assert_eq!(diag.group, Some(load_group.id));
            let Some(reason) = diag
                .message
                .strip_prefix("the settings file cannot be read: ")
            else {
                panic!("unexpected diagnostic message: {}", diag.message);
            };
            assert!(!reason.trim().is_empty());

            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        assert!(dir_path.is_dir());
        assert_eq!(fs::read_to_string(&invalid_json_path).unwrap(), "not json");
        assert_eq!(fs::read_to_string(&array_json_path).unwrap(), "[1, 2, 3]");

        // 3. Valid file: `loaded {sections=4}`, `from=file`, `from=defaults`, broken sections
        //    warn inside `settings-register{section}` before `registered {from=defaults}`,
        //    nothing logged for unknown keys, and duplicates log `ERROR`.
        let valid_path = temp.path().join("settings.json");
        fs::write(
            &valid_path,
            r#"{
                "undeclared_section": {"ignored": true},
                "first": {"greeting": "hi", "size": 20},
                "third": {"flag": "wrong"},
                "fourth": []
            }"#,
        )
        .unwrap();

        let app = cx.new_app();
        app.update(|cx| {
            init(cx);
            load(cx, valid_path.clone());
            register::<First>(cx);
            register::<Second>(cx);
            register::<Third>(cx);
            register::<Fourth>(cx);
            register::<First>(cx);
            register::<FirstDuplicate>(cx);
        });
        app.read(|cx| {
            let first = get::<First>(cx);
            assert_eq!(first.greeting, "hi");
            assert_eq!(first.size, 20);
            assert!(first.enabled);
            let second = get::<Second>(cx);
            assert_eq!(second.label, "panel");
            assert_eq!(second.limit, 3);
            let third = get::<Third>(cx);
            assert!(!third.flag);
            let fourth = get::<Fourth>(cx);
            assert_eq!(fourth.count, 5);
        });

        let captured = CAPTURED.lock().unwrap();
        let [
            init_group,
            load_group,
            first_group,
            second_group,
            third_group,
            fourth_group,
            dup_same_group,
            dup_other_group,
        ] = &captured.groups[groups_cursor..]
        else {
            panic!(
                "expected 8 groups, got {:#?}",
                &captured.groups[groups_cursor..]
            );
        };
        assert_init_group(init_group, &captured.entered_groups);
        assert_load_group(load_group, &captured.entered_groups, &valid_path);
        assert_register_group(first_group, &captured.entered_groups, "first");
        assert_register_group(second_group, &captured.entered_groups, "second");
        assert_register_group(third_group, &captured.entered_groups, "third");
        assert_register_group(fourth_group, &captured.entered_groups, "fourth");
        assert_register_group(dup_same_group, &captured.entered_groups, "first");
        assert_register_group(dup_other_group, &captured.entered_groups, "first");

        let [
            store_created_event,
            loaded_event,
            first_registered,
            second_registered,
            third_registered,
            fourth_registered,
        ] = &captured.events[events_cursor..]
        else {
            panic!(
                "expected 6 events, got {:#?}",
                &captured.events[events_cursor..]
            );
        };
        assert_debug_event(store_created_event, init_group.id, "store created", &[]);
        assert_debug_event(loaded_event, load_group.id, "loaded", &[("sections", "4")]);
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
            &[("from", "defaults")],
        );
        assert_debug_event(
            third_registered,
            third_group.id,
            "registered",
            &[("from", "defaults")],
        );
        assert_debug_event(
            fourth_registered,
            fourth_group.id,
            "registered",
            &[("from", "defaults")],
        );

        let [
            third_section_warning,
            fourth_section_warning,
            duplicate_same_type_error,
            duplicate_other_type_error,
        ] = &captured.diagnostics[diagnostics_cursor..]
        else {
            panic!(
                "expected 4 diagnostics, got {:#?}",
                &captured.diagnostics[diagnostics_cursor..]
            );
        };

        assert_eq!(third_section_warning.level, log::Level::Warn);
        assert_eq!(third_section_warning.group, Some(third_group.id));
        let Some(third_reason) = third_section_warning
            .message
            .strip_prefix("section third cannot be read: ")
        else {
            panic!(
                "unexpected diagnostic message: {}",
                third_section_warning.message
            );
        };
        assert!(!third_reason.trim().is_empty());
        assert!(
            third_section_warning.seq < third_registered.seq,
            "section warning must precede registered {{from=defaults}}"
        );

        assert_eq!(fourth_section_warning.level, log::Level::Warn);
        assert_eq!(fourth_section_warning.group, Some(fourth_group.id));
        let Some(fourth_reason) = fourth_section_warning
            .message
            .strip_prefix("section fourth cannot be read: ")
        else {
            panic!(
                "unexpected diagnostic message: {}",
                fourth_section_warning.message
            );
        };
        assert!(!fourth_reason.trim().is_empty());
        assert!(
            fourth_section_warning.seq < fourth_registered.seq,
            "section warning must precede registered {{from=defaults}}"
        );

        assert_eq!(duplicate_same_type_error.level, log::Level::Error);
        assert_eq!(duplicate_same_type_error.group, Some(dup_same_group.id));
        assert_eq!(
            duplicate_same_type_error.message,
            "section first is already registered"
        );

        assert_eq!(duplicate_other_type_error.level, log::Level::Error);
        assert_eq!(duplicate_other_type_error.group, Some(dup_other_group.id));
        assert_eq!(
            duplicate_other_type_error.message,
            "section first is already registered"
        );

        for group in &captured.groups {
            assert!(
                !group
                    .fields
                    .values()
                    .any(|v| v.contains("undeclared_section")),
                "unknown key leaked into group: {group:#?}"
            );
        }
        for event in &captured.events {
            assert!(
                event
                    .message
                    .as_deref()
                    .is_none_or(|m| !m.contains("undeclared_section"))
                    && !event
                        .fields
                        .values()
                        .any(|v| v.contains("undeclared_section")),
                "unknown key leaked into event: {event:#?}"
            );
        }
        for diagnostic in &captured.diagnostics {
            assert!(
                !diagnostic.message.contains("undeclared_section"),
                "unknown key leaked into diagnostic: {diagnostic:#?}"
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
