use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
use yard_settings::{
    SchemaWrite, Settings, describe, get, init, load, register, reset, reset_value,
    write_schema_file, write_value,
};

const SECRET_GREETING: &str = "distinctive-secret-greeting-7b9e4a";
const SECOND_GREETING: &str = "distinctive-second-greeting-3c8d1f";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-schema-logging-{}-{}-{nanos:x}",
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
    group: Option<u64>,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Diagnostic {
    level: log::Level,
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
        CAPTURED.lock().unwrap().diagnostics.push(Diagnostic {
            level: record.level(),
            message: record.args().to_string(),
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

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        if let Some(group) = captured
            .groups
            .iter_mut()
            .find(|group| group.id == span.into_u64())
        {
            group.fields.extend(fields.values);
        }
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Name,
    Date,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Advanced {
    flag: bool,
}

/// Test section
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    /// Show the preview.
    preview: bool,
    /// Sort order.
    sort: Order,
    /// Items per page.
    #[schemars(range(min = 1, max = 100))]
    page_size: u32,
    /// Greeting.
    greeting: String,
    /// Nested.
    advanced: Advanced,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            preview: false,
            sort: Order::Name,
            page_size: 10,
            greeting: "hi".into(),
            advanced: Advanced::default(),
        }
    }
}

impl Settings for Sample {
    const KEY: &'static str = "sample";
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Unregistered {
    flag: bool,
}

impl Settings for Unregistered {
    const KEY: &'static str = "unregistered";
}

fn clean_field(value: &str) -> &str {
    value.trim_matches('"')
}

fn assert_no_secret_logged(captured: &Captured) {
    for secret in [SECRET_GREETING, SECOND_GREETING] {
        for group in &captured.groups {
            assert!(
                !group.name.contains(secret),
                "group name leaked setting value: {group:#?}"
            );
            for (name, value) in &group.fields {
                assert!(
                    !name.contains(secret) && !value.contains(secret),
                    "group field leaked setting value: {group:#?}"
                );
            }
        }
        for event in &captured.events {
            if let Some(message) = &event.message {
                assert!(
                    !message.contains(secret),
                    "event message leaked setting value: {event:#?}"
                );
            }
            for (name, value) in &event.fields {
                assert!(
                    !name.contains(secret) && !value.contains(secret),
                    "event field leaked setting value: {event:#?}"
                );
            }
        }
        for diagnostic in &captured.diagnostics {
            assert!(
                diagnostic.level != log::Level::Error,
                "unexpected ERROR diagnostic: {diagnostic:#?}"
            );
            assert!(
                !diagnostic.message.contains(secret),
                "diagnostic leaked setting value: {diagnostic:#?}"
            );
        }
    }
}

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn schema_and_reset_operations_log_contract_groups_and_never_log_values(cx: &mut TestAppContext) {
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    let schema_path = temp.path().join("settings.schema.json");
    fs::write(
        &settings_path,
        serde_json::to_vec(&json!({
            "sample": {
                "greeting": SECRET_GREETING,
                "preview": true
            }
        }))
        .unwrap(),
    )
    .unwrap();

    tracing::subscriber::with_default(Trace, || {
        cx.update(|cx| {
            init(cx);
            load(cx, settings_path.clone());
            register::<Sample>(cx);
        });
        cx.read(|cx| assert_eq!(get::<Sample>(cx).greeting, SECRET_GREETING));

        let (mut groups_cursor, mut events_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            (captured.groups.len(), captured.events.len())
        };

        // 1. `settings-describe{section}` with `described`
        cx.update(describe::<Sample>);
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            let describe_group = groups
                .iter()
                .find(|group| group.name == "settings-describe")
                .unwrap_or_else(|| panic!("expected settings-describe group in {groups:#?}"));
            assert!(captured.entered_groups.contains(&describe_group.id));
            assert_eq!(describe_group.level, tracing::Level::INFO);
            assert_eq!(
                describe_group.fields.get("section").map(|s| clean_field(s)),
                Some("sample"),
                "unexpected settings-describe fields: {:#?}",
                describe_group.fields
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.group == Some(describe_group.id)
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("described")
                }),
                "expected DEBUG 'described' inside settings-describe, got {events:#?}"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
        }

        // 2. `settings-schema-write{sections}` with `written`, then `unchanged`
        let first_write = cx
            .read(|cx| write_schema_file(cx, &schema_path))
            .expect("first write_schema_file succeeds");
        assert_eq!(first_write, SchemaWrite::Written);
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            let schema_group = groups
                .iter()
                .find(|group| group.name == "settings-schema-write")
                .unwrap_or_else(|| panic!("expected settings-schema-write group in {groups:#?}"));
            assert!(captured.entered_groups.contains(&schema_group.id));
            assert_eq!(schema_group.level, tracing::Level::INFO);
            assert_eq!(
                schema_group.fields.get("sections").map(|s| clean_field(s)),
                Some("1"),
                "unexpected settings-schema-write fields: {:#?}",
                schema_group.fields
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.group == Some(schema_group.id)
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("written")
                }),
                "expected DEBUG 'written' inside settings-schema-write, got {events:#?}"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
        }

        let second_write = cx
            .read(|cx| write_schema_file(cx, &schema_path))
            .expect("second write_schema_file succeeds");
        assert_eq!(second_write, SchemaWrite::Unchanged);
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            let schema_group = groups
                .iter()
                .find(|group| group.name == "settings-schema-write")
                .unwrap_or_else(|| panic!("expected settings-schema-write group in {groups:#?}"));
            assert!(captured.entered_groups.contains(&schema_group.id));
            assert_eq!(schema_group.level, tracing::Level::INFO);
            assert_eq!(
                schema_group.fields.get("sections").map(|s| clean_field(s)),
                Some("1"),
                "unexpected settings-schema-write fields: {:#?}",
                schema_group.fields
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.group == Some(schema_group.id)
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("unchanged")
                }),
                "expected DEBUG 'unchanged' inside settings-schema-write, got {events:#?}"
            );
        }

        // Write a second distinctive greeting, then reset it so both write_value and reset run on it.
        cx.update(|cx| write_value(cx, "sample", "greeting", json!(SECOND_GREETING)));
        cx.read(|cx| assert_eq!(get::<Sample>(cx).greeting, SECOND_GREETING));
        {
            let captured = CAPTURED.lock().unwrap();
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
        }

        // 3. `settings-reset{section, field}` with `reset`
        cx.update(|cx| reset::<Sample>(cx, "greeting"));
        cx.read(|cx| assert_eq!(get::<Sample>(cx).greeting, "hi"));
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            let reset_group = groups
                .iter()
                .find(|group| group.name == "settings-reset")
                .unwrap_or_else(|| panic!("expected settings-reset group in {groups:#?}"));
            assert!(captured.entered_groups.contains(&reset_group.id));
            assert_eq!(reset_group.level, tracing::Level::INFO);
            assert_eq!(
                reset_group.fields.get("section").map(|s| clean_field(s)),
                Some("sample")
            );
            assert_eq!(
                reset_group.fields.get("field").map(|s| clean_field(s)),
                Some("greeting")
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.group == Some(reset_group.id)
                        && event.level == tracing::Level::DEBUG
                        && event.message.as_deref() == Some("reset")
                }),
                "expected DEBUG 'reset' inside settings-reset, got {events:#?}"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
        }

        // 4. `settings-reset{section, field}` refused (`info` `refused` with `reason`) for unknown field & unknown section
        cx.update(|cx| reset::<Sample>(cx, "no_such_field"));
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            let reset_group = groups
                .iter()
                .find(|group| group.name == "settings-reset")
                .unwrap_or_else(|| panic!("expected settings-reset group in {groups:#?}"));
            assert!(captured.entered_groups.contains(&reset_group.id));
            assert_eq!(reset_group.level, tracing::Level::INFO);
            assert_eq!(
                reset_group.fields.get("section").map(|s| clean_field(s)),
                Some("sample")
            );
            assert_eq!(
                reset_group.fields.get("field").map(|s| clean_field(s)),
                Some("no_such_field")
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.group == Some(reset_group.id)
                        && event.level == tracing::Level::INFO
                        && event.message.as_deref() == Some("refused")
                        && event
                            .fields
                            .get("reason")
                            .is_some_and(|reason| !clean_field(reason).trim().is_empty())
                }),
                "expected INFO 'refused' with non-empty reason inside settings-reset, got {events:#?}"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
        }

        cx.update(|cx| reset::<Unregistered>(cx, "flag"));
        cx.update(|cx| reset_value(cx, "unregistered", "flag"));
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = &captured.groups[groups_cursor..];
            assert!(
                groups.iter().any(|group| {
                    group.name == "settings-reset"
                        && group.fields.get("section").map(|s| clean_field(s))
                            == Some("unregistered")
                        && group.fields.get("field").map(|s| clean_field(s)) == Some("flag")
                }),
                "expected settings-reset group for unregistered section, got {groups:#?}"
            );
            let events = &captured.events[events_cursor..];
            assert!(
                events.iter().any(|event| {
                    event.level == tracing::Level::INFO
                        && event.message.as_deref() == Some("refused")
                        && event
                            .fields
                            .get("reason")
                            .is_some_and(|reason| !clean_field(reason).trim().is_empty())
                }),
                "expected INFO 'refused' with reason for unregistered section, got {events:#?}"
            );
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
    assert_no_secret_logged(&captured);
}
