//!
//! Secret store operations run on GPUI's background executor (which may run on
//! other threads), so this file installs one process-wide tracing subscriber and
//! diagnostic logger, and its single test runs alone in this binary.

use gpui::TestAppContext;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Mutex;
use yard_shell::{self as shell, Extension, MAX_SECRET_BYTES, SecretError, SecretStore};

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

struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, SpanRecord>,
    step_span_ids: Vec<u64>,
    step_events: Vec<EventRecord>,
    all_text: Vec<String>,
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured {
    next_id: 1,
    spans: BTreeMap::new(),
    step_span_ids: Vec::new(),
    step_events: Vec::new(),
    all_text: Vec::new(),
});

thread_local! {
    static ENTERED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

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

struct Trace;

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = FieldMap::default();
        attributes.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        let id = captured.next_id;
        captured.next_id += 1;
        let name = attributes.metadata().name().to_string();
        captured.all_text.push(name.clone());
        for (k, v) in &fields.0 {
            captured.all_text.push(format!("{k}={v}"));
        }
        let record = SpanRecord {
            id,
            level: *attributes.metadata().level(),
            name,
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
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        for (k, v) in &fields.0 {
            captured.all_text.push(format!("{k}={v}"));
        }
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
        let mut span_stack = ENTERED.with(|stack| stack.borrow().clone());
        if let Some(parent) = event.parent() {
            let parent_id = parent.into_u64();
            if !span_stack.contains(&parent_id) {
                span_stack.push(parent_id);
            }
        }
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        captured.all_text.push(message.clone());
        for (k, v) in &fields.0 {
            captured.all_text.push(format!("{k}={v}"));
        }
        captured.step_events.push(EventRecord {
            level: *event.metadata().level(),
            span_stack,
            message,
            fields: fields.0,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        ENTERED.with(|stack| stack.borrow_mut().push(id.into_u64()));
    }

    fn exit(&self, id: &tracing::span::Id) {
        ENTERED.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(pos) = stack.iter().rposition(|&item| item == id.into_u64()) {
                stack.remove(pos);
            }
        });
    }
}

fn take_step() -> (Vec<SpanRecord>, Vec<EventRecord>, Vec<(log::Level, String)>) {
    let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
    let ids = std::mem::take(&mut captured.step_span_ids);
    let spans = ids
        .into_iter()
        .filter_map(|id| captured.spans.get(&id).cloned())
        .filter(|span| span.name == "secret")
        .collect();
    let events = std::mem::take(&mut captured.step_events);
    let diagnostics = {
        let mut diag = DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner());
        let taken = std::mem::take(&mut *diag);
        for (_, message) in &taken {
            captured.all_text.push(message.clone());
        }
        taken
    };
    (spans, events, diagnostics)
}

fn assert_secret_span(spans: &[SpanRecord], expected_extension: &str, expected_op: &str) -> u64 {
    assert_eq!(
        spans.len(),
        1,
        "expected exactly one `secret` span for op={expected_op}, got {spans:#?}"
    );
    let span = &spans[0];
    assert_eq!(
        span.level,
        tracing::Level::INFO,
        "`secret` must be an info_span: {span:#?}"
    );
    assert_eq!(
        span.fields.get("extension").map(String::as_str),
        Some(expected_extension),
        "expected extension={expected_extension:?} on `secret` span: {span:#?}"
    );
    assert_eq!(
        span.fields.get("op").map(String::as_str),
        Some(expected_op),
        "expected op={expected_op:?} on `secret` span: {span:#?}"
    );
    span.id
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

#[gpui::test]
async fn secret_operations_log_groups_levels_and_never_leak_key_or_value(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let key = "user-token-xyz";
    let bad_key = "bad user-token-xyz/secret";
    let value = b"s3cr3t-value";
    let oversized: Vec<u8> = b"s3cr3t-value"
        .iter()
        .copied()
        .cycle()
        .take(MAX_SECRET_BYTES + 1)
        .collect();

    let store = SecretStore::memory();
    let app = cx.new_app();
    let ext = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "vault").unwrap()
    });
    let _ = take_step();

    // 1. Successful set: secret{extension="vault", op="set"} with debug "stored".
    app.read(|cx| ext.set_secret(key, value, cx)).await.unwrap();
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "set");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "stored"),
        "expected DEBUG `stored` inside secret{{op=set}}, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 2. Successful get (found = true): secret{extension="vault", op="get"} with debug "read" found=true.
    let loaded = app
        .read(|cx| ext.get_secret(key, cx))
        .await
        .unwrap()
        .expect("entry must exist");
    assert_eq!(loaded.expose(), value);
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "get");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::DEBUG
                && e.message == "read"
                && e.fields.get("found").map(String::as_str) == Some("true")
        }),
        "expected DEBUG `read` with found=true inside secret{{op=get}}, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 3. Successful delete: secret{extension="vault", op="delete"} with debug "deleted".
    app.read(|cx| ext.delete_secret(key, cx)).await.unwrap();
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "delete");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "deleted"),
        "expected DEBUG `deleted` inside secret{{op=delete}}, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 4. Successful get of missing entry (found = false): debug "read" found=false.
    assert!(
        app.read(|cx| ext.get_secret(key, cx))
            .await
            .unwrap()
            .is_none()
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "get");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::DEBUG
                && e.message == "read"
                && e.fields.get("found").map(String::as_str) == Some("false")
        }),
        "expected DEBUG `read` with found=false inside secret{{op=get}}, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 5. Refusal InvalidKey: info "refused" with reason="invalid secret key".
    assert_eq!(
        app.read(|cx| ext.set_secret(bad_key, value, cx)).await,
        Err(SecretError::InvalidKey)
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "set");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::INFO
                && e.message == "refused"
                && e.fields.get("reason").map(String::as_str) == Some("invalid secret key")
        }),
        "expected INFO `refused` with reason='invalid secret key', got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 6. Refusal TooLarge: info "refused" with reason="secret too large".
    assert_eq!(
        app.read(|cx| ext.set_secret(key, &oversized, cx)).await,
        Err(SecretError::TooLarge)
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "set");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::INFO
                && e.message == "refused"
                && e.fields.get("reason").map(String::as_str) == Some("secret too large")
        }),
        "expected INFO `refused` with reason='secret too large', got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 7. Denied: info "denied", no warn/error, no diagnostic log.
    store.fail_next(SecretError::Denied);
    assert_eq!(
        app.read(|cx| ext.get_secret(key, cx)).await.err(),
        Some(SecretError::Denied)
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "get");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::INFO
                && e.message == "denied"
                && e.fields.get("reason").map(String::as_str)
                    == Some("access to the secret store denied")
        }),
        "expected INFO `denied` with reason='access to the secret store denied', got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 8. Unavailable: WARN inside group + one log::warn!, no ERROR.
    store.fail_next(SecretError::Unavailable);
    assert_eq!(
        app.read(|cx| ext.get_secret(key, cx)).await.err(),
        Some(SecretError::Unavailable)
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "get");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| e.level == tracing::Level::WARN),
        "expected WARN inside secret{{op=get}} on Unavailable, got {in_span:#?}"
    );
    assert!(
        !events.iter().any(|e| e.level == tracing::Level::ERROR),
        "Unavailable must not emit tracing ERROR, got {events:#?}"
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
        "Unavailable must emit exactly one log::warn!, got {diagnostics:#?}"
    );
    assert!(
        diag_errors.is_empty(),
        "Unavailable must not emit log::error!, got {diagnostics:#?}"
    );
    assert!(
        diag_warns[0]
            .1
            .contains(&SecretError::Unavailable.to_string()),
        "log::warn! on Unavailable should carry the fixed text, got {:?}",
        diag_warns[0].1
    );

    // 9. Failed: WARN inside group + one log::warn!, no ERROR.
    store.fail_next(SecretError::Failed);
    assert_eq!(
        app.read(|cx| ext.set_secret(key, value, cx)).await,
        Err(SecretError::Failed)
    );
    let (spans, events, diagnostics) = take_step();
    let span_id = assert_secret_span(&spans, "vault", "set");
    let in_span = events_in_span(&events, span_id);
    assert!(
        in_span.iter().any(|e| e.level == tracing::Level::WARN),
        "expected WARN inside secret{{op=set}} on Failed, got {in_span:#?}"
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
        "Failed must emit exactly one log::warn!, got {diagnostics:#?}"
    );
    assert!(
        diag_errors.is_empty(),
        "Failed must not emit log::error!, got {diagnostics:#?}"
    );
    assert!(
        diag_warns[0].1.contains(&SecretError::Failed.to_string()),
        "log::warn! on Failed should carry the fixed text, got {:?}",
        diag_warns[0].1
    );

    // Privacy: no span, trace event, or diagnostic line holds the key or the value.
    let all_text = CAPTURED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .all_text
        .clone();
    for needle in [key, bad_key, "s3cr3t-value"] {
        assert!(
            all_text.iter().all(|line| !line.contains(needle)),
            "log output must not contain `{needle}`: {all_text:#?}"
        );
    }

    ENTERED.with(|stack| {
        assert!(
            stack.borrow().is_empty(),
            "every entered span on the main thread must exit cleanly"
        );
    });

    app.quit();
    app.run_until_parked();
}
