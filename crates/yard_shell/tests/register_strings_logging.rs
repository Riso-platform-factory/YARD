//! Logging tests for `Extension::register_strings` reserved-namespace refusals.
//!
//! Alone in this binary: installs the process-wide diagnostic logger and captures
//! tracing under one scoped subscriber so no parallel test can race on tracing's
//! callsite interest cache.

use gpui::TestAppContext;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_i18n::{CatalogError, init, set_system_locale_for_tests};
use yard_shell::Extension;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-register-strings-logging-{}-{}-{nanos:x}",
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

#[derive(Clone, Debug)]
struct EventRecord {
    target: String,
    level: tracing::Level,
    message: String,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct Captured {
    events: Vec<EventRecord>,
    diagnostics: Vec<String>,
}

struct Step {
    events: Vec<EventRecord>,
    diagnostics: Vec<String>,
}

static INIT_LOGGER: Once = Once::new();
static ACTIVE_CAPTURE: Mutex<Option<Arc<Mutex<Captured>>>> = Mutex::new(None);

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell") || metadata.target().starts_with("yard_i18n")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = ACTIVE_CAPTURE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(captured) = target {
            captured
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .diagnostics
                .push(format!(
                    "{} {}: {}",
                    record.level(),
                    record.target(),
                    record.args()
                ));
        }
    }

    fn flush(&self) {}
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

impl Trace {
    fn take_step(&self) -> Step {
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Step {
            events: std::mem::take(&mut captured.events),
            diagnostics: std::mem::take(&mut captured.diagnostics),
        }
    }
}

#[derive(Default)]
struct FieldMap(BTreeMap<String, String>);

impl tracing::field::Visit for FieldMap {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_owned(),
            format!("{value:?}").trim_matches('"').to_owned(),
        );
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
        let metadata = event.metadata();
        let target = metadata.target();
        if !target.starts_with("yard_shell") && !target.starts_with("yard_i18n") {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .events
            .push(EventRecord {
                target: target.to_owned(),
                level: *metadata.level(),
                message,
                fields: fields.0,
            });
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[gpui::test]
fn reserved_namespace_registration_logs_info_with_reason_and_no_errors(cx: &mut TestAppContext) {
    // Setting a tracing dispatcher before `log::set_logger` keeps `tracing`'s
    // `log` fallback from mirroring trace lines into `log` on Linux.
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
    INIT_LOGGER.call_once(|| {
        log::set_logger(&Diagnostics).expect("install diagnostic logger");
        log::set_max_level(log::LevelFilter::Trace);
    });

    let trace = Trace::default();
    *ACTIVE_CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(trace.0.clone());

    let dir = TempDir::new();
    let path = dir.path().join("settings.json");
    fs::write(&path, "{}").unwrap();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            yard_settings::init(cx);
            yard_settings::load(cx, path);
            init(cx);
            set_system_locale_for_tests(cx, None);
        });
        let _ = trace.take_step();

        for namespace in ["app", "yard_ui"] {
            let ext = cx.update(|cx| Extension::new(cx, namespace).unwrap());
            let _ = trace.take_step();

            let Err(err) =
                cx.update(|cx| ext.register_strings(cx, "en", r#"{"title": "Hijacked"}"#))
            else {
                panic!("reserved namespace {namespace:?} must be refused");
            };
            assert!(
                matches!(err, CatalogError::ReservedNamespace),
                "expected ReservedNamespace for {namespace:?}, got {err:?}"
            );

            let step = trace.take_step();
            let info_events: Vec<_> = step
                .events
                .iter()
                .filter(|event| {
                    event.target.starts_with("yard_shell") && event.level == tracing::Level::INFO
                })
                .collect();
            assert_eq!(
                info_events.len(),
                1,
                "reserved namespace {namespace:?} must emit one info! event: {:#?}",
                step.events
            );
            assert!(
                info_events[0]
                    .message
                    .to_ascii_lowercase()
                    .contains("reserved"),
                "info! for {namespace:?} must state the reserved-namespace reason: {:#?}",
                info_events[0]
            );
            assert_eq!(
                info_events[0].fields.get("namespace").map(String::as_str),
                Some(namespace),
                "info! for {namespace:?} must record the namespace field: {:#?}",
                info_events[0]
            );
            assert_eq!(
                info_events[0].fields.get("locale").map(String::as_str),
                Some("en"),
                "info! for {namespace:?} must record the locale field: {:#?}",
                info_events[0]
            );

            let unexpected_trace: Vec<_> = step
                .events
                .iter()
                .filter(|event| matches!(event.level, tracing::Level::WARN | tracing::Level::ERROR))
                .collect();
            assert!(
                unexpected_trace.is_empty(),
                "reserved namespace refusal for {namespace:?} must not emit WARN/ERROR tracing events: {unexpected_trace:#?}"
            );
            assert!(
                step.diagnostics.is_empty(),
                "reserved namespace refusal for {namespace:?} must not emit diagnostic log records: {:#?}",
                step.diagnostics
            );
        }
    });

    *ACTIVE_CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
