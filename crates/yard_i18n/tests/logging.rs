#![cfg(feature = "dev")]

//! Lifecycle trace and diagnostic log tests for `yard_i18n/catalog`.
//!
//! Alone in this binary: installs the process-wide diagnostic logger and captures
//! tracing under one scoped subscriber so no parallel test can race on tracing's
//! callsite interest cache.

use gpui::TestAppContext;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_i18n::{CatalogError, init, locale, register, set_system_locale_for_tests, t};
use yard_settings::WriteOutcome;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-i18n-logging-{}-{}-{nanos:x}",
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
struct SpanRecord {
    id: u64,
    level: tracing::Level,
    name: String,
    opening_fields: BTreeMap<String, String>,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct EventRecord {
    level: tracing::Level,
    span_stack: Vec<u64>,
    message: String,
}

#[derive(Clone, Debug)]
struct DiagnosticRecord {
    level: log::Level,
    span_stack: Vec<u64>,
    message: String,
}

struct Step {
    spans: Vec<SpanRecord>,
    events: Vec<EventRecord>,
    lifecycle_events: Vec<EventRecord>,
    diagnostics: Vec<DiagnosticRecord>,
}

#[derive(Default)]
struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
    entered_spans: BTreeSet<u64>,
    step_span_ids: Vec<u64>,
    step_events: Vec<EventRecord>,
    step_lifecycle_events: Vec<EventRecord>,
    step_diagnostics: Vec<DiagnosticRecord>,
}

static INIT_LOGGER: Once = Once::new();
static ACTIVE_CAPTURE: Mutex<Option<Arc<Mutex<Captured>>>> = Mutex::new(None);

fn log_to_tracing_level(level: log::Level) -> tracing::Level {
    match level {
        log::Level::Error => tracing::Level::ERROR,
        log::Level::Warn => tracing::Level::WARN,
        log::Level::Info => tracing::Level::INFO,
        log::Level::Debug => tracing::Level::DEBUG,
        log::Level::Trace => tracing::Level::TRACE,
    }
}

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_i18n")
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
            let mut captured = captured.lock().unwrap_or_else(|e| e.into_inner());
            let span_stack = captured.entered.clone();
            let message = record.args().to_string();
            captured.step_diagnostics.push(DiagnosticRecord {
                level: record.level(),
                span_stack: span_stack.clone(),
                message: message.clone(),
            });
            // Model `yard_log::Logger::write`, which forwards every `log` record
            // into the lifecycle trace via `tracing_log::format_trace`.
            captured.step_lifecycle_events.push(EventRecord {
                level: log_to_tracing_level(record.level()),
                span_stack,
                message,
            });
        }
    }

    fn flush(&self) {}
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

impl Trace {
    fn take_step(&self) -> Step {
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let span_ids = std::mem::take(&mut captured.step_span_ids);
        let spans = span_ids
            .into_iter()
            .filter_map(|id| captured.spans.get(&id).cloned())
            .collect();
        let events = std::mem::take(&mut captured.step_events);
        let lifecycle_events = std::mem::take(&mut captured.step_lifecycle_events);
        let diagnostics = std::mem::take(&mut captured.step_diagnostics);
        Step {
            spans,
            events,
            lifecycle_events,
            diagnostics,
        }
    }

    fn was_entered(&self, span_id: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entered_spans
            .contains(&span_id)
    }
}

#[derive(Default)]
struct FieldMap(BTreeMap<String, String>);

impl tracing::field::Visit for FieldMap {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        self.0.insert(field.name().to_owned(), value.to_string());
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        self.0.insert(field.name().to_owned(), value.to_string());
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

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = FieldMap::default();
        attributes.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        captured.next_id += 1;
        let id = captured.next_id;
        let metadata = attributes.metadata();
        let record = SpanRecord {
            id,
            level: *metadata.level(),
            name: metadata.name().to_owned(),
            opening_fields: fields.0.clone(),
            fields: fields.0,
        };
        captured.spans.insert(id, record);
        if metadata.target().starts_with("yard_i18n") {
            captured.step_span_ids.push(id);
        }
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = FieldMap::default();
        values.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = captured.spans.get_mut(&span.into_u64()) {
            record.fields.extend(fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("yard_i18n") {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let span_stack = captured.entered.clone();
        let record = EventRecord {
            level: *metadata.level(),
            span_stack,
            message,
        };
        captured.step_events.push(record.clone());
        captured.step_lifecycle_events.push(record);
    }

    fn enter(&self, span: &tracing::span::Id) {
        let id = span.into_u64();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        captured.entered.push(id);
        captured.entered_spans.insert(id);
    }

    fn exit(&self, span: &tracing::span::Id) {
        let id = span.into_u64();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = captured.entered.iter().rposition(|&open| open == id) {
            captured.entered.remove(pos);
        }
    }
}

fn events_in_span(events: &[EventRecord], span_id: u64) -> Vec<&EventRecord> {
    events
        .iter()
        .filter(|event| event.span_stack.contains(&span_id))
        .collect()
}

fn assert_no_errors(events: &[EventRecord], diagnostics: &[DiagnosticRecord]) {
    let trace_errors: Vec<_> = events
        .iter()
        .filter(|event| event.level == tracing::Level::ERROR)
        .collect();
    let diag_errors: Vec<_> = diagnostics
        .iter()
        .filter(|record| record.level == log::Level::Error)
        .collect();
    assert!(
        trace_errors.is_empty() && diag_errors.is_empty(),
        "unexpected errors: trace={trace_errors:#?}, diagnostics={diag_errors:#?}"
    );
}

#[gpui::test]
fn logging_covers_registration_locale_apply_and_missing_key_warnings(cx: &mut TestAppContext) {
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

        // 0. Before any catalog is registered, requesting `en` or `en-US` must never
        //    emit the "no catalog" info message.
        for tag in ["en", "en-US"] {
            let outcome =
                cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!(tag)));
            assert_eq!(outcome, WriteOutcome::Written);
            cx.run_until_parked();
            let step = trace.take_step();
            let info_events: Vec<_> = step
                .lifecycle_events
                .iter()
                .filter(|event| event.level == tracing::Level::INFO)
                .collect();
            assert!(
                info_events.is_empty(),
                "no \"no catalog\" info! may be emitted for {tag:?}: {info_events:#?}"
            );
            assert_no_errors(&step.events, &step.diagnostics);
        }
        let outcome =
            cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!(null)));
        assert_eq!(outcome, WriteOutcome::Written);
        cx.run_until_parked();
        let _ = trace.take_step();

        // 1. catalog-register{namespace, locale, keys} with `keys` as an opening field
        //    and debug!("registered").
        cx.update(|cx| {
            register(
                cx,
                "app",
                "en",
                r#"{"hello": "Hello", "only_en": "Only English"}"#,
            )
            .unwrap();
        });
        let step = trace.take_step();
        let register_span = step
            .spans
            .iter()
            .find(|span| span.name == "catalog-register")
            .unwrap_or_else(|| panic!("expected catalog-register span, got {:#?}", step.spans));
        assert_eq!(register_span.level, tracing::Level::INFO);
        assert!(
            trace.was_entered(register_span.id),
            "catalog-register span must be entered while registration runs"
        );
        assert_eq!(
            register_span
                .opening_fields
                .get("namespace")
                .map(String::as_str),
            Some("app"),
            "catalog-register opening fields: {:#?}",
            register_span.opening_fields
        );
        assert_eq!(
            register_span
                .opening_fields
                .get("locale")
                .map(String::as_str),
            Some("en"),
            "catalog-register opening fields: {:#?}",
            register_span.opening_fields
        );
        assert_eq!(
            register_span.opening_fields.get("keys").map(String::as_str),
            Some("2"),
            "catalog-register must supply `keys` as an opening field (yard_log has no on_record): opening={:#?}, all={:#?}",
            register_span.opening_fields,
            register_span.fields
        );
        let in_register = events_in_span(&step.events, register_span.id);
        assert!(
            in_register
                .iter()
                .any(|event| event.level == tracing::Level::DEBUG && event.message == "registered"),
            "expected debug!(\"registered\") inside catalog-register, got {in_register:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);
        assert!(
            step.diagnostics.is_empty(),
            "valid catalog registration must not emit diagnostics: {:#?}",
            step.diagnostics
        );

        cx.update(|cx| {
            register(cx, "app", "vi", r#"{"hello": "Xin chào"}"#).unwrap();
        });
        let step = trace.take_step();
        let vi_span = step
            .spans
            .iter()
            .find(|span| span.name == "catalog-register")
            .unwrap_or_else(|| {
                panic!(
                    "expected catalog-register span for vi, got {:#?}",
                    step.spans
                )
            });
        assert_eq!(
            vi_span.opening_fields.get("locale").map(String::as_str),
            Some("vi")
        );
        assert_eq!(
            vi_span.opening_fields.get("keys").map(String::as_str),
            Some("1"),
            "catalog-register must supply `keys` as an opening field: opening={:#?}, all={:#?}",
            vi_span.opening_fields,
            vi_span.fields
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 2. Malformed catalog logs ERROR inside catalog-register and never logs "registered".
        let err = cx
            .update(|cx| register(cx, "app", "en", r#"{"good": "No", "bad": 42}"#))
            .expect_err("malformed catalog must be refused");
        match err {
            CatalogError::BadValue { key } => assert_eq!(key, "bad"),
            other => panic!("expected BadValue {{ key: \"bad\" }}, got {other:?}"),
        }
        let step = trace.take_step();
        let bad_span = step
            .spans
            .iter()
            .find(|span| span.name == "catalog-register")
            .unwrap_or_else(|| {
                panic!(
                    "expected catalog-register span on failure, got {:#?}",
                    step.spans
                )
            });
        assert_eq!(bad_span.level, tracing::Level::INFO);
        assert!(trace.was_entered(bad_span.id));
        let in_bad = events_in_span(&step.events, bad_span.id);
        assert!(
            !in_bad.iter().any(|event| event.message == "registered"),
            "malformed catalog must not log \"registered\": {in_bad:#?}"
        );
        let trace_errors: Vec<_> = in_bad
            .iter()
            .filter(|event| event.level == tracing::Level::ERROR)
            .collect();
        let diag_errors: Vec<_> = step
            .diagnostics
            .iter()
            .filter(|record| {
                record.level == log::Level::Error && record.span_stack.contains(&bad_span.id)
            })
            .collect();
        assert!(
            trace_errors.len() <= 1
                && diag_errors.len() <= 1
                && (trace_errors.len() + diag_errors.len()) >= 1,
            "malformed catalog must emit one ERROR inside catalog-register: trace={trace_errors:#?}, diagnostics={diag_errors:#?}"
        );
        for record in &diag_errors {
            assert!(
                !record.message.is_empty(),
                "diagnostic error must carry a message"
            );
        }

        // 3. `fi -> es -> fi` in one update emits the "no catalog" info! exactly once,
        //    inside `locale-apply`.
        cx.update(|cx| {
            set_system_locale_for_tests(cx, Some("fi"));
            set_system_locale_for_tests(cx, Some("es"));
            set_system_locale_for_tests(cx, Some("fi"));
        });
        cx.run_until_parked();
        let step = trace.take_step();
        let info_events: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::INFO)
            .collect();
        assert_eq!(
            info_events.len(),
            1,
            "fi -> es -> fi in one update must emit the \"no catalog\" message once: {info_events:#?}"
        );
        assert!(
            step.spans.iter().any(|span| {
                span.name == "locale-apply" && info_events[0].span_stack.contains(&span.id)
            }),
            "\"no catalog\" info! must be emitted inside locale-apply: event={:#?}, spans={:#?}",
            info_events[0],
            step.spans
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 3b. Requesting `FI` in a later update must not emit a second "no catalog" notice.
        cx.update(|cx| set_system_locale_for_tests(cx, Some("FI")));
        cx.run_until_parked();
        let step = trace.take_step();
        let info_events: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::INFO)
            .collect();
        assert!(
            info_events.is_empty(),
            "FI after fi must not emit a second \"no catalog\" notice: {info_events:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 3c. An unchanged request opens no `locale-apply` span.
        cx.update(|cx| set_system_locale_for_tests(cx, Some("FI")));
        cx.run_until_parked();
        let step = trace.take_step();
        assert!(
            !step.spans.iter().any(|span| span.name == "locale-apply"),
            "unchanged request must not open a locale-apply span: {:#?}",
            step.spans
        );
        let info_events: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::INFO)
            .collect();
        assert!(
            info_events.is_empty(),
            "unchanged request must not emit info events: {info_events:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 3d. Resetting to `None` and then requesting `fi` again emits one fresh notice.
        cx.update(|cx| set_system_locale_for_tests(cx, None));
        cx.run_until_parked();
        let step = trace.take_step();
        let info_events: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::INFO)
            .collect();
        assert!(
            info_events.is_empty(),
            "resetting to None must not emit a \"no catalog\" notice: {info_events:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        cx.update(|cx| set_system_locale_for_tests(cx, Some("fi")));
        cx.run_until_parked();
        let step = trace.take_step();
        let info_events: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::INFO)
            .collect();
        assert_eq!(
            info_events.len(),
            1,
            "fi after None must emit one fresh \"no catalog\" notice: {info_events:#?}"
        );
        assert!(
            step.spans.iter().any(|span| {
                span.name == "locale-apply" && info_events[0].span_stack.contains(&span.id)
            }),
            "\"no catalog\" info! after reset must be emitted inside locale-apply: event={:#?}, spans={:#?}",
            info_events[0],
            step.spans
        );
        assert_no_errors(&step.events, &step.diagnostics);

        cx.update(|cx| set_system_locale_for_tests(cx, None));
        cx.run_until_parked();
        let _ = trace.take_step();

        // 4. locale-apply{requested, resolved} with debug!("applied").
        let outcome =
            cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi")));
        assert_eq!(outcome, WriteOutcome::Written);
        cx.read(|cx| assert_eq!(locale(cx).as_str(), "vi"));
        let step = trace.take_step();
        let apply_span = step
            .spans
            .iter()
            .find(|span| span.name == "locale-apply")
            .unwrap_or_else(|| panic!("expected locale-apply span, got {:#?}", step.spans));
        assert_eq!(apply_span.level, tracing::Level::INFO);
        assert!(
            trace.was_entered(apply_span.id),
            "locale-apply span must be entered while applying the locale"
        );
        let requested = apply_span
            .opening_fields
            .get("requested")
            .unwrap_or_else(|| {
                panic!(
                    "locale-apply missing opening `requested`: {:#?}",
                    apply_span.opening_fields
                )
            });
        assert!(
            requested.contains("vi"),
            "expected requested field to name \"vi\", got {requested:?}"
        );
        assert_eq!(
            apply_span
                .opening_fields
                .get("resolved")
                .map(String::as_str),
            Some("vi"),
            "locale-apply opening fields: {:#?}",
            apply_span.opening_fields
        );
        let in_apply = events_in_span(&step.events, apply_span.id);
        assert!(
            in_apply
                .iter()
                .any(|event| event.level == tracing::Level::DEBUG && event.message == "applied"),
            "expected debug!(\"applied\") inside locale-apply, got {in_apply:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 5. Found lookups are trace! only (never debug/info/warn/error).
        cx.read(|cx| assert_eq!(t(cx, "app.hello").as_str(), "Xin chào"));
        let step = trace.take_step();
        let non_trace: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level != tracing::Level::TRACE)
            .collect();
        assert!(
            non_trace.is_empty(),
            "found lookup must log at trace! only, got {non_trace:#?}"
        );
        assert!(
            step.diagnostics.is_empty(),
            "found lookup must not write diagnostics, got {:#?}",
            step.diagnostics
        );

        // 6. Two lookups of a missing key in `vi` (falls back to `en`) give
        //    1 diagnostic warning and 2 lifecycle warnings, not 3 (since `yard_log`
        //    forwards `log::warn!` into the lifecycle trace).
        cx.read(|cx| {
            for _ in 0..2 {
                assert_eq!(t(cx, "app.only_en").as_str(), "Only English");
            }
        });
        let step = trace.take_step();
        let diag_warns: Vec<_> = step
            .diagnostics
            .iter()
            .filter(|record| record.level == log::Level::Warn)
            .collect();
        let lifecycle_warns: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::WARN)
            .collect();
        assert_eq!(
            diag_warns.len(),
            1,
            "two lookups of a missing key must emit one diagnostic warning: {:#?}",
            step.diagnostics
        );
        assert_eq!(
            lifecycle_warns.len(),
            2,
            "two lookups of a missing key must emit two lifecycle warnings, not three: lifecycle={lifecycle_warns:#?}, tracing={:#?}, diagnostics={:#?}",
            step.events,
            step.diagnostics
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 7. Missing key everywhere in `vi`: 1 diagnostic warning and 3 lifecycle warnings over 3 lookups.
        cx.read(|cx| {
            for _ in 0..3 {
                assert_eq!(t(cx, "app.nowhere").as_str(), "app.nowhere");
            }
        });
        let step = trace.take_step();
        let diag_warns: Vec<_> = step
            .diagnostics
            .iter()
            .filter(|record| record.level == log::Level::Warn)
            .collect();
        let lifecycle_warns: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::WARN)
            .collect();
        assert_eq!(
            diag_warns.len(),
            1,
            "missing key everywhere in vi must log one diagnostic warning over three lookups: {:#?}",
            step.diagnostics
        );
        assert_eq!(
            lifecycle_warns.len(),
            3,
            "missing key everywhere in vi must log three lifecycle warnings over three lookups: {lifecycle_warns:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 8. Switching locale to `en` gives one fresh diagnostic warning for ("en", "app.nowhere")
        //    and 3 lifecycle warnings over 3 lookups.
        let outcome =
            cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("en")));
        assert_eq!(outcome, WriteOutcome::Written);
        cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
        let _ = trace.take_step();

        cx.read(|cx| {
            for _ in 0..3 {
                assert_eq!(t(cx, "app.nowhere").as_str(), "app.nowhere");
            }
        });
        let step = trace.take_step();
        let diag_warns: Vec<_> = step
            .diagnostics
            .iter()
            .filter(|record| record.level == log::Level::Warn)
            .collect();
        let lifecycle_warns: Vec<_> = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::WARN)
            .collect();
        assert_eq!(
            diag_warns.len(),
            1,
            "log::warn! deduplication is per locale and key: {:#?}",
            step.diagnostics
        );
        assert_eq!(
            lifecycle_warns.len(),
            3,
            "missing key in en must log three lifecycle warnings over three lookups: {lifecycle_warns:#?}"
        );
        assert_no_errors(&step.events, &step.diagnostics);

        // 9. A key with no namespace dot returns the key and emits 1 diagnostic warning and 1 lifecycle warning.
        cx.read(|cx| assert_eq!(t(cx, "nodot").as_str(), "nodot"));
        let step = trace.take_step();
        let diag_warns = step
            .diagnostics
            .iter()
            .filter(|record| record.level == log::Level::Warn)
            .count();
        let lifecycle_warns = step
            .lifecycle_events
            .iter()
            .filter(|event| event.level == tracing::Level::WARN)
            .count();
        assert_eq!(
            diag_warns, 1,
            "key without namespace dot must emit one diagnostic warning: {:#?}",
            step.diagnostics
        );
        assert_eq!(
            lifecycle_warns, 1,
            "key without namespace dot must emit one lifecycle warning: {:#?}",
            step.lifecycle_events
        );
        assert_no_errors(&step.events, &step.diagnostics);
    });

    *ACTIVE_CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let captured = trace.0.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        captured.entered.is_empty(),
        "every entered span must exit cleanly: {:?}",
        captured.entered
    );
}
