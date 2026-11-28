#![cfg(feature = "dev")]

use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_settings::{Settings, describe, get, init, load, register, watch, write, write_value};

const SENTINEL_LOAD_NUM: &str = "sentinel-load-num-84d2a1";
const SENTINEL_WRITE_NUM: &str = "sentinel-write-num-19f3c7";
const SENTINEL_WRITE_ENUM: &str = "sentinel-write-enum-52b8e4";
const SENTINEL_WRITE_VALUE_NUM: &str = "sentinel-write-value-num-63c9d0";
const SENTINEL_WRITE_VALUE_ENUM: &str = "sentinel-write-value-enum-71a4f8";
const SENTINEL_RELOAD_NUM: &str = "sentinel-reload-num-38e5b2";
const SENTINEL_RELOAD_ENUM: &str = "sentinel-reload-enum-90d7c6";

const SENTINELS: [&str; 7] = [
    SENTINEL_LOAD_NUM,
    SENTINEL_WRITE_NUM,
    SENTINEL_WRITE_ENUM,
    SENTINEL_WRITE_VALUE_NUM,
    SENTINEL_WRITE_VALUE_ENUM,
    SENTINEL_RELOAD_NUM,
    SENTINEL_RELOAD_ENUM,
];

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-no-values-in-logs-{}-{}-{nanos:x}",
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
struct SpanRecord {
    name: &'static str,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct EventRecord {
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct DiagnosticRecord {
    message: String,
}

struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, SpanRecord>,
    events: Vec<EventRecord>,
    diagnostics: Vec<DiagnosticRecord>,
}

impl Captured {
    const fn new() -> Self {
        Self {
            next_id: 1,
            spans: BTreeMap::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            CAPTURED.lock().unwrap().diagnostics.push(DiagnosticRecord {
                message: record.args().to_string(),
            });
        }
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

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
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
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        if attributes.metadata().target().starts_with("yard") {
            captured.spans.insert(
                id,
                SpanRecord {
                    name: attributes.metadata().name(),
                    fields: fields.values,
                },
            );
        }
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if let Some(record) = CAPTURED.lock().unwrap().spans.get_mut(&span.into_u64()) {
            record.fields.extend(fields.values);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        CAPTURED.lock().unwrap().events.push(EventRecord {
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Name,
    Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    sort: Order,
    page_size: u32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            sort: Order::Name,
            page_size: 10,
        }
    }
}

impl Settings for Sample {
    const KEY: &'static str = "sample";
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn decode_failures_in_write_write_value_and_reload_never_log_values(cx: &mut TestAppContext) {
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "sample": {
                "page_size": SENTINEL_LOAD_NUM
            }
        }))
        .unwrap(),
    )
    .unwrap();

    tracing::subscriber::with_default(Trace, || {
        // 1. Initial `load` + `register` on a file holding an undecodable sentinel value.
        cx.update(|cx| {
            init(cx);
            load(cx, path.clone());
            register::<Sample>(cx);
            describe::<Sample>(cx);
            watch(cx);
        });
        cx.read(|cx| assert_eq!(get::<Sample>(cx), &Sample::default()));

        // Restore a valid file before testing `write` and `write_value` decode failures.
        fs::write(&path, "{}\n").unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();

        // 2. `write` with undecodable sentinel values.
        cx.update(|cx| {
            write::<Sample>(cx, "page_size", json!(SENTINEL_WRITE_NUM));
            write::<Sample>(cx, "sort", json!(SENTINEL_WRITE_ENUM));
        });

        // 3. `write_value` with undecodable sentinel values.
        cx.update(|cx| {
            let _ = write_value(cx, "sample", "page_size", json!(SENTINEL_WRITE_VALUE_NUM));
            let _ = write_value(cx, "sample", "sort", json!(SENTINEL_WRITE_VALUE_ENUM));
        });

        // 4. Reloading `settings.json` holding undecodable sentinel values.
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "sample": {
                    "page_size": SENTINEL_RELOAD_NUM
                }
            }))
            .unwrap(),
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();

        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "sample": {
                    "sort": SENTINEL_RELOAD_ENUM
                }
            }))
            .unwrap(),
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        !captured.events.is_empty() && !captured.diagnostics.is_empty(),
        "expected both tracing events and diagnostic logs to be captured"
    );

    let mut leaks = Vec::new();
    for sentinel in SENTINELS {
        for span in captured.spans.values() {
            if span.name.contains(sentinel) {
                leaks.push(format!("span name leaked {sentinel}: {span:?}"));
            }
            for (name, value) in &span.fields {
                if name.contains(sentinel) || value.contains(sentinel) {
                    leaks.push(format!("span field leaked {sentinel}: {span:?}"));
                }
            }
        }
        for event in &captured.events {
            if let Some(message) = &event.message
                && message.contains(sentinel)
            {
                leaks.push(format!("event message leaked {sentinel}: {event:?}"));
            }
            for (name, value) in &event.fields {
                if name.contains(sentinel) || value.contains(sentinel) {
                    leaks.push(format!("event field leaked {sentinel}: {event:?}"));
                }
            }
        }
        for diagnostic in &captured.diagnostics {
            if diagnostic.message.contains(sentinel) {
                leaks.push(format!("diagnostic leaked {sentinel}: {diagnostic:?}"));
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "setting values leaked into logs:\n{}",
        leaks.join("\n")
    );
}
