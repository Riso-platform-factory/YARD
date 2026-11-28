use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_settings::{
    Settings, WriteOutcome, describe, described_sections, get, init, json_schema, load, register,
    reset, reset_value, write, write_value,
};

static TEST_LOCK: Mutex<()> = Mutex::new(());
static LOGGER_INIT: OnceLock<()> = OnceLock::new();
static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static TRACE_EVENTS: Mutex<Vec<(tracing::Level, String)>> = Mutex::new(Vec::new());

fn test_lock() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

struct WarningTrace;

#[derive(Default)]
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_owned();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for WarningTrace {
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
        if !event.metadata().target().starts_with("yard_settings") {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        TRACE_EVENTS
            .lock()
            .unwrap()
            .push((*event.metadata().level(), visitor.0));
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

struct CapturedLogs {
    diagnostics: Vec<(log::Level, String)>,
    traces: Vec<(tracing::Level, String)>,
}

fn capture_logs<R>(f: impl FnOnce() -> R) -> (R, CapturedLogs) {
    LOGGER_INIT.get_or_init(|| {
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    DIAGNOSTICS.lock().unwrap().clear();
    TRACE_EVENTS.lock().unwrap().clear();
    let out = tracing::subscriber::with_default(WarningTrace, || {
        tracing::callsite::rebuild_interest_cache();
        f()
    });
    let diagnostics = DIAGNOSTICS.lock().unwrap().clone();
    let traces = TRACE_EVENTS.lock().unwrap().clone();
    (
        out,
        CapturedLogs {
            diagnostics,
            traces,
        },
    )
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-reset-{}-{}-{nanos:x}",
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

fn read_object(path: &Path) -> Map<String, Value> {
    let text = fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[gpui::test]
fn reset_removes_field_preserves_key_order_removes_empty_section_and_updates_store(
    cx: &mut TestAppContext,
) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(
        &path,
        r#"{"other": {"x": 1}, "sample": {"preview": true, "page_size": 20}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                page_size: 20,
                ..Sample::default()
            }
        );
    });

    cx.update(|cx| reset::<Sample>(cx, "preview"));

    let after_first = read_object(&path);
    assert_eq!(
        after_first.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["other", "sample"]
    );
    assert_eq!(
        Value::Object(after_first),
        json!({"other": {"x": 1}, "sample": {"page_size": 20}})
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: false,
                page_size: 20,
                ..Sample::default()
            }
        );
    });

    cx.update(|cx| reset::<Sample>(cx, "page_size"));

    let after_second = read_object(&path);
    assert_eq!(
        after_second.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["other"]
    );
    assert_eq!(Value::Object(after_second), json!({"other": {"x": 1}}));
    cx.read(|cx| assert_eq!(get::<Sample>(cx), &Sample::default()));
}

#[gpui::test]
fn unknown_field_is_refused_and_missing_field_does_not_write(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
    let before_bytes = fs::read(&path).unwrap();
    let before_mtime = fs::metadata(&path).unwrap().modified().unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
    });

    // A field the section does not have is refused and leaves the file untouched.
    cx.update(|cx| reset::<Sample>(cx, "unknown_field"));
    assert_eq!(fs::read(&path).unwrap(), before_bytes);
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        before_mtime
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                ..Sample::default()
            }
        );
    });

    // A valid field missing from the file succeeds without rewriting the file.
    cx.update(|cx| reset::<Sample>(cx, "page_size"));
    assert_eq!(fs::read(&path).unwrap(), before_bytes);
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        before_mtime
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                ..Sample::default()
            }
        );
    });
}

#[gpui::test]
fn write_value_and_reset_value_update_the_file_and_published_section(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(
        &path,
        r#"{"other": {"x": 1}, "sample": {"preview": true, "page_size": 20}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    cx.update(|cx| {
        let _ = reset_value(cx, "sample", "preview");
    });
    let after_reset = read_object(&path);
    assert_eq!(
        after_reset.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["other", "sample"]
    );
    assert_eq!(
        Value::Object(after_reset),
        json!({"other": {"x": 1}, "sample": {"page_size": 20}})
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: false,
                page_size: 20,
                ..Sample::default()
            }
        );
    });

    cx.update(|cx| {
        let _ = write_value(cx, "sample", "page_size", json!(30));
    });
    let after_write = read_object(&path);
    assert_eq!(
        after_write.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["other", "sample"]
    );
    assert_eq!(
        Value::Object(after_write),
        json!({"other": {"x": 1}, "sample": {"page_size": 30}})
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: false,
                page_size: 30,
                ..Sample::default()
            }
        );
    });

    cx.update(|cx| {
        let _ = reset_value(cx, "sample", "page_size");
    });
    let after_clear = read_object(&path);
    assert_eq!(
        after_clear.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["other"]
    );
    assert_eq!(Value::Object(after_clear), json!({"other": {"x": 1}}));
    cx.read(|cx| assert_eq!(get::<Sample>(cx), &Sample::default()));
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Impostor {
    preview: bool,
    extra: u32,
}

impl Settings for Impostor {
    const KEY: &'static str = "sample";
}

#[gpui::test]
fn malformed_settings_json_stays_byte_for_byte_after_write_reset_write_value_and_reset_value(
    cx: &mut TestAppContext,
) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true, "page_size": 20}}"#).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let broken = b"{ not json";
    fs::write(&path, broken).unwrap();

    cx.update(|cx| write::<Sample>(cx, "page_size", json!(30)));
    assert_eq!(fs::read(&path).unwrap(), broken);

    cx.update(|cx| reset::<Sample>(cx, "preview"));
    assert_eq!(fs::read(&path).unwrap(), broken);

    cx.update(|cx| {
        let _ = write_value(cx, "sample", "page_size", json!(30));
    });
    assert_eq!(fs::read(&path).unwrap(), broken);

    cx.update(|cx| {
        let _ = reset_value(cx, "sample", "preview");
    });
    assert_eq!(fs::read(&path).unwrap(), broken);

    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                page_size: 20,
                ..Sample::default()
            }
        );
    });

    // Also when the file was already malformed when `load` ran.
    let fresh_path = temp.path().join("broken-at-load.json");
    fs::write(&fresh_path, broken).unwrap();
    let app = cx.new_app();
    app.update(|cx| {
        init(cx);
        load(cx, fresh_path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
        write::<Sample>(cx, "page_size", json!(30));
    });
    assert_eq!(fs::read(&fresh_path).unwrap(), broken);
    app.update(|cx| reset::<Sample>(cx, "preview"));
    assert_eq!(fs::read(&fresh_path).unwrap(), broken);
    app.update(|cx| {
        let _ = write_value(cx, "sample", "page_size", json!(30));
    });
    assert_eq!(fs::read(&fresh_path).unwrap(), broken);
    app.update(|cx| {
        let _ = reset_value(cx, "sample", "preview");
    });
    assert_eq!(fs::read(&fresh_path).unwrap(), broken);
    app.read(|cx| assert_eq!(get::<Sample>(cx), &Sample::default()));
}

#[gpui::test]
fn dollar_schema_at_top_is_kept_by_write_and_reset(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(
        &path,
        r#"{"$schema": "./settings.schema.json", "sample": {"preview": true}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });
    cx.read(|cx| assert!(get::<Sample>(cx).preview));

    cx.update(|cx| write::<Sample>(cx, "page_size", json!(25)));
    let after_write = read_object(&path);
    assert_eq!(
        after_write.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["$schema", "sample"]
    );
    assert_eq!(
        Value::Object(after_write),
        json!({
            "$schema": "./settings.schema.json",
            "sample": {"preview": true, "page_size": 25}
        })
    );

    cx.update(|cx| reset::<Sample>(cx, "preview"));
    let after_reset = read_object(&path);
    assert_eq!(
        after_reset.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["$schema", "sample"]
    );
    assert_eq!(
        Value::Object(after_reset),
        json!({
            "$schema": "./settings.schema.json",
            "sample": {"page_size": 25}
        })
    );

    cx.update(|cx| reset::<Sample>(cx, "page_size"));
    let after_empty_section = read_object(&path);
    assert_eq!(
        after_empty_section
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["$schema"]
    );
    assert_eq!(
        Value::Object(after_empty_section),
        json!({"$schema": "./settings.schema.json"})
    );
}

#[gpui::test]
fn disk_edits_between_yard_writes_and_resets_are_kept(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
        write::<Sample>(cx, "preview", json!(true));
    });

    // External edit on disk between two YARD writes (with no watch tick).
    fs::write(
        &path,
        r#"{"$schema": "./settings.schema.json", "sample": {"preview": true, "greeting": "from disk"}, "extra": {"x": 1}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        let _ = write_value(cx, "sample", "page_size", json!(40));
    });
    assert_eq!(
        Value::Object(read_object(&path)),
        json!({
            "$schema": "./settings.schema.json",
            "sample": {
                "preview": true,
                "greeting": "from disk",
                "page_size": 40
            },
            "extra": {"x": 1}
        })
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                greeting: "from disk".into(),
                page_size: 40,
                ..Sample::default()
            }
        );
    });

    // Another external edit before a reset; both changes are kept.
    fs::write(
        &path,
        r#"{"$schema": "./settings.schema.json", "sample": {"preview": true, "greeting": "second disk edit", "page_size": 40}, "extra": {"x": 2}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        let _ = reset_value(cx, "sample", "preview");
    });
    assert_eq!(
        Value::Object(read_object(&path)),
        json!({
            "$schema": "./settings.schema.json",
            "sample": {
                "greeting": "second disk edit",
                "page_size": 40
            },
            "extra": {"x": 2}
        })
    );
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: false,
                greeting: "second disk edit".into(),
                page_size: 40,
                ..Sample::default()
            }
        );
    });
}

#[gpui::test]
fn describe_and_reset_refuse_wrong_type_registered_under_same_key(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();
    let before = fs::read(&path).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Impostor>(cx);
    });

    cx.read(|cx| {
        let sections = described_sections(cx);
        assert!(
            sections.is_empty(),
            "describe::<Impostor> after register::<Sample> must be refused"
        );
        assert!(
            json_schema(cx)["properties"].get("sample").is_none(),
            "refused describe must not add section to json_schema"
        );
    });

    cx.update(|cx| reset::<Impostor>(cx, "preview"));
    assert_eq!(fs::read(&path).unwrap(), before);
    cx.read(|cx| assert!(get::<Sample>(cx).preview));
}

#[gpui::test]
fn reset_against_non_object_section_refuses_and_logs_exactly_one_warn(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let non_object = br#"{"sample": 5}"#;
    fs::write(&path, non_object).unwrap();

    let (outcome, reset_value_logs) =
        capture_logs(|| cx.update(|cx| reset_value(cx, "sample", "preview")));
    assert!(
        matches!(outcome, WriteOutcome::Refused(_)),
        "reset_value against non-object section must refuse, got {outcome:?}"
    );
    assert_eq!(fs::read(&path).unwrap(), non_object);
    let log_warns: Vec<_> = reset_value_logs
        .diagnostics
        .iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .collect();
    let log_errors: Vec<_> = reset_value_logs
        .diagnostics
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .collect();
    assert!(
        log_errors.is_empty(),
        "unexpected error diagnostics: {log_errors:?}"
    );
    assert_eq!(
        log_warns.len(),
        1,
        "reset_value against non-object section must log exactly one log::warn!, got: {:?}",
        reset_value_logs.diagnostics
    );

    let ((), reset_logs) = capture_logs(|| cx.update(|cx| reset::<Sample>(cx, "preview")));
    assert_eq!(fs::read(&path).unwrap(), non_object);
    let typed_warns: Vec<_> = reset_logs
        .diagnostics
        .iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .collect();
    let typed_errors: Vec<_> = reset_logs
        .diagnostics
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .collect();
    assert!(
        typed_errors.is_empty(),
        "unexpected error diagnostics: {typed_errors:?}"
    );
    assert_eq!(
        typed_warns.len(),
        1,
        "reset::<Sample> against non-object section must log exactly one log::warn!, got: {:?}",
        reset_logs.diagnostics
    );
}

#[gpui::test]
fn register_describe_type_mismatch_refuses_with_no_warn(cx: &mut TestAppContext) {
    let _guard = test_lock();
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();
    let before = fs::read(&path).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
    });

    let ((), describe_logs) = capture_logs(|| cx.update(describe::<Impostor>));
    cx.read(|cx| {
        assert!(described_sections(cx).is_empty());
        assert!(json_schema(cx)["properties"].get("sample").is_none());
    });
    assert!(
        describe_logs
            .diagnostics
            .iter()
            .all(|(level, _)| *level > log::Level::Warn),
        "register::<Sample>/describe::<Impostor> mismatch must log no diagnostic warn/error, got: {:?}",
        describe_logs.diagnostics
    );
    assert!(
        describe_logs
            .traces
            .iter()
            .all(|(level, _)| *level > tracing::Level::WARN),
        "register::<Sample>/describe::<Impostor> mismatch must log no trace warn/error, got: {:?}",
        describe_logs.traces
    );

    let ((), reset_logs) = capture_logs(|| cx.update(|cx| reset::<Impostor>(cx, "preview")));
    assert_eq!(fs::read(&path).unwrap(), before);
    cx.read(|cx| assert!(get::<Sample>(cx).preview));
    assert!(
        reset_logs
            .diagnostics
            .iter()
            .all(|(level, _)| *level > log::Level::Warn),
        "register::<Sample>/reset::<Impostor> mismatch must log no diagnostic warn/error, got: {:?}",
        reset_logs.diagnostics
    );
    assert!(
        reset_logs
            .traces
            .iter()
            .all(|(level, _)| *level > tracing::Level::WARN),
        "register::<Sample>/reset::<Impostor> mismatch must log no trace warn/error, got: {:?}",
        reset_logs.traces
    );
}
