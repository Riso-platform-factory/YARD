//! Lifecycle trace and diagnostic log tests for `yard_shell/autostart`.
//!
//! Alone in this binary: captures tracing output under one scoped subscriber
//! and installs the process-wide diagnostic logger so no other test can race on
//! tracing's callsite interest cache.

use gpui::TestAppContext;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Once},
};
use yard_shell::{self as shell, AutostartBackend, AutostartError, AutostartStatus, Extension};

static INIT_LOGGER: Once = Once::new();
static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    level: tracing::Level,
    name: String,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct EventRecord {
    level: tracing::Level,
    span_stack: Vec<u64>,
    message: String,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
    step_span_ids: Vec<u64>,
    step_events: Vec<EventRecord>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct FieldMap(BTreeMap<String, String>);

impl tracing::field::Visit for FieldMap {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_u128(&mut self, field: &tracing::field::Field, value: u128) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_i128(&mut self, field: &tracing::field::Field, value: i128) {
        self.0.insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_string(),
            format!("{value:?}").trim_matches('"').to_string(),
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
        let record = SpanRecord {
            id,
            level: *attributes.metadata().level(),
            name: attributes.metadata().name().to_string(),
            fields: fields.0,
        };
        if attributes.metadata().target().starts_with("yard_shell") {
            captured.step_span_ids.push(id);
        }
        captured.spans.insert(id, record);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = FieldMap::default();
        values.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(span) = captured.spans.get_mut(&id.into_u64()) {
            span.fields.extend(fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_shell") {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.get("message").cloned().unwrap_or_default();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut span_stack = captured.entered.clone();
        if let Some(parent) = event.parent() {
            let parent_id = parent.into_u64();
            if !span_stack.contains(&parent_id) {
                span_stack.push(parent_id);
            }
        }
        captured.step_events.push(EventRecord {
            level: *event.metadata().level(),
            span_stack,
            message,
            fields: fields.0,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entered
            .push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entered
                .pop(),
            Some(id.into_u64())
        );
    }
}

impl Trace {
    fn take_step(&self) -> (Vec<SpanRecord>, Vec<EventRecord>, Vec<(log::Level, String)>) {
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let ids = std::mem::take(&mut captured.step_span_ids);
        let spans = ids
            .into_iter()
            .filter_map(|id| captured.spans.get(&id).cloned())
            .filter(|span| span.name.starts_with("autostart-"))
            .collect();
        let events = std::mem::take(&mut captured.step_events);
        let diagnostics =
            std::mem::take(&mut *DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()));
        (spans, events, diagnostics)
    }
}

fn events_in_span(events: &[EventRecord], span_id: u64) -> Vec<&EventRecord> {
    events
        .iter()
        .filter(|event| event.span_stack.contains(&span_id))
        .collect()
}

fn assert_no_warn_or_error(events: &[EventRecord], diagnostics: &[(log::Level, String)]) {
    let bad_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.level, tracing::Level::WARN | tracing::Level::ERROR))
        .collect();
    assert!(
        bad_events.is_empty(),
        "expected no WARN or ERROR tracing events, got {bad_events:#?}"
    );
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostic log records, got {diagnostics:#?}"
    );
}

fn assert_autostart_span(
    spans: &[SpanRecord],
    expected_name: &str,
    expected_extension: &str,
) -> SpanRecord {
    assert_eq!(
        spans.len(),
        1,
        "expected exactly one `{expected_name}` span, got {spans:#?}"
    );
    let span = &spans[0];
    assert_eq!(
        span.name, expected_name,
        "expected `{expected_name}` span, got {span:#?}"
    );
    assert_eq!(
        span.level,
        tracing::Level::INFO,
        "`{expected_name}` must be an info_span: {span:#?}"
    );
    assert_eq!(
        span.fields.get("extension").map(String::as_str),
        Some(expected_extension),
        "expected extension={expected_extension:?} on `{expected_name}` span: {span:#?}"
    );
    span.clone()
}

#[gpui::test]
fn autostart_operations_log_groups_fields_and_levels(cx: &mut TestAppContext) {
    INIT_LOGGER.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });

    let backend = AutostartBackend::memory();
    let notes = cx.update(|cx| {
        shell::set_autostart_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    let trace = Trace::default();
    let _ = trace.take_step();

    tracing::subscriber::with_default(trace.clone(), || {
        // 1. Read initial status: autostart-status{extension="notes"} with debug "checked" and status.
        assert_eq!(
            cx.read(|cx| notes.launch_at_login(cx)),
            Ok(AutostartStatus::Disabled)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-status", "notes");
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::DEBUG
                    && e.message == "checked"
                    && e.fields
                        .get("status")
                        .is_some_and(|s| s.eq_ignore_ascii_case("disabled"))
            }),
            "expected DEBUG `checked` with status=`disabled`/`Disabled` inside autostart-status, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 2. Enable: autostart-set{extension="notes", enabled=true} with debug "updated" and status.
        assert_eq!(
            cx.read(|cx| notes.set_launch_at_login(true, cx)),
            Ok(AutostartStatus::Enabled)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-set", "notes");
        assert_eq!(
            span.fields.get("enabled").map(String::as_str),
            Some("true"),
            "expected enabled=true on autostart-set span: {span:#?}"
        );
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::DEBUG
                    && e.message == "updated"
                    && e.fields
                        .get("status")
                        .is_some_and(|s| s.eq_ignore_ascii_case("enabled"))
            }),
            "expected DEBUG `updated` with status=`enabled`/`Enabled` inside autostart-set, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 3. Read enabled status: autostart-status{extension="notes"} with debug "checked" and status.
        assert_eq!(
            cx.read(|cx| notes.launch_at_login(cx)),
            Ok(AutostartStatus::Enabled)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-status", "notes");
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::DEBUG
                    && e.message == "checked"
                    && e.fields
                        .get("status")
                        .is_some_and(|s| s.eq_ignore_ascii_case("enabled"))
            }),
            "expected DEBUG `checked` with status=`enabled`/`Enabled` inside autostart-status, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 4. Disable: autostart-set{extension="notes", enabled=false} with debug "updated".
        assert_eq!(
            cx.read(|cx| notes.set_launch_at_login(false, cx)),
            Ok(AutostartStatus::Disabled)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-set", "notes");
        assert_eq!(
            span.fields.get("enabled").map(String::as_str),
            Some("false"),
            "expected enabled=false on autostart-set span: {span:#?}"
        );
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::DEBUG
                    && e.message == "updated"
                    && e.fields
                        .get("status")
                        .is_some_and(|s| s.eq_ignore_ascii_case("disabled"))
            }),
            "expected DEBUG `updated` with status=`disabled`/`Disabled` inside autostart-set, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 5. Refusal NotSupported: INFO "refused" with reason, no WARN/ERROR or diagnostic log.
        backend.set_status(AutostartStatus::NotSupported);
        assert_eq!(
            cx.read(|cx| notes.set_launch_at_login(true, cx)),
            Err(AutostartError::NotSupported)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-set", "notes");
        assert_eq!(
            span.fields.get("enabled").map(String::as_str),
            Some("true"),
            "expected enabled=true on autostart-set span: {span:#?}"
        );
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::INFO
                    && e.message == "refused"
                    && e.fields.get("reason").is_some_and(|r| !r.is_empty())
            }),
            "expected INFO `refused` with reason on NotSupported, got {in_span:#?}"
        );
        assert!(
            !in_span
                .iter()
                .any(|e| e.level == tracing::Level::DEBUG && e.message == "updated"),
            "refused NotSupported must not log DEBUG `updated`, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 6. Refusal NeedsApproval: INFO "refused" with reason, no WARN/ERROR or diagnostic log.
        backend.set_status(AutostartStatus::Disabled);
        backend.fail_next(AutostartError::NeedsApproval);
        assert_eq!(
            cx.read(|cx| notes.set_launch_at_login(true, cx)),
            Err(AutostartError::NeedsApproval)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-set", "notes");
        assert_eq!(
            span.fields.get("enabled").map(String::as_str),
            Some("true"),
            "expected enabled=true on autostart-set span: {span:#?}"
        );
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| {
                e.level == tracing::Level::INFO
                    && e.message == "refused"
                    && e.fields.get("reason").is_some_and(|r| !r.is_empty())
            }),
            "expected INFO `refused` with reason on NeedsApproval, got {in_span:#?}"
        );
        assert!(
            !in_span
                .iter()
                .any(|e| e.level == tracing::Level::DEBUG && e.message == "updated"),
            "refused NeedsApproval must not log DEBUG `updated`, got {in_span:#?}"
        );
        assert_no_warn_or_error(&events, &diagnostics);

        // 7. Failed on set_launch_at_login: WARN inside autostart-set + one log::warn!, no ERROR.
        backend.fail_next(AutostartError::Failed);
        assert_eq!(
            cx.read(|cx| notes.set_launch_at_login(true, cx)),
            Err(AutostartError::Failed)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-set", "notes");
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| e.level == tracing::Level::WARN),
            "expected WARN inside autostart-set on Failed, got {in_span:#?}"
        );
        assert!(
            !events.iter().any(|e| e.level == tracing::Level::ERROR),
            "Failed must not emit tracing ERROR, got {events:#?}"
        );
        let diag_warns: Vec<_> = diagnostics
            .iter()
            .filter(|(level, _)| *level == log::Level::Warn)
            .collect();
        let diag_errors: Vec<_> = diagnostics
            .iter()
            .filter(|(level, _)| *level == log::Level::Error)
            .collect();
        assert_eq!(
            diag_warns.len(),
            1,
            "Failed on set_launch_at_login must emit one log::warn!, got {diagnostics:#?}"
        );
        assert!(
            diag_errors.is_empty(),
            "Failed on set_launch_at_login must not emit log::error!, got {diagnostics:#?}"
        );

        // 8. Failed on launch_at_login: WARN inside autostart-status + one log::warn!, no ERROR.
        backend.fail_next(AutostartError::Failed);
        assert_eq!(
            cx.read(|cx| notes.launch_at_login(cx)),
            Err(AutostartError::Failed)
        );
        let (spans, events, diagnostics) = trace.take_step();
        let span = assert_autostart_span(&spans, "autostart-status", "notes");
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| e.level == tracing::Level::WARN),
            "expected WARN inside autostart-status on Failed, got {in_span:#?}"
        );
        assert!(
            !events.iter().any(|e| e.level == tracing::Level::ERROR),
            "Failed on launch_at_login must not emit tracing ERROR, got {events:#?}"
        );
        let diag_warns: Vec<_> = diagnostics
            .iter()
            .filter(|(level, _)| *level == log::Level::Warn)
            .collect();
        let diag_errors: Vec<_> = diagnostics
            .iter()
            .filter(|(level, _)| *level == log::Level::Error)
            .collect();
        assert_eq!(
            diag_warns.len(),
            1,
            "Failed on launch_at_login must emit one log::warn!, got {diagnostics:#?}"
        );
        assert!(
            diag_errors.is_empty(),
            "Failed on launch_at_login must not emit log::error!, got {diagnostics:#?}"
        );
    });

    let captured = trace.0.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        captured.entered.is_empty(),
        "every entered span must exit cleanly"
    );
}
