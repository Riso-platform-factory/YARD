#![cfg(feature = "dev")]

// Alone in this binary: captures tracing output under one scoped subscriber and
// installs the process-wide diagnostic logger so no other test can race on
// tracing's callsite interest cache.

use gpui::TestAppContext;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, Once};
use yard_shell::{Extension, ExternalOpenError, MAX_URL_LEN};

static INIT_LOGGER: Once = Once::new();
static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    level: tracing::Level,
    target: String,
    name: String,
    declared_fields: Vec<String>,
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
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
    events: Vec<EventRecord>,
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
        let declared_fields = attributes
            .metadata()
            .fields()
            .iter()
            .map(|field| field.name().to_string())
            .collect();
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let id = captured.spans.len() as u64 + 1;
        captured.spans.insert(
            id,
            SpanRecord {
                id,
                level: *attributes.metadata().level(),
                target: attributes.metadata().target().to_string(),
                name: attributes.metadata().name().to_string(),
                declared_fields,
                fields: fields.0,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !target.starts_with("yard_shell") {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.get("message").cloned().unwrap_or_default();
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let span_stack = captured.entered.clone();
        captured.events.push(EventRecord {
            level: *event.metadata().level(),
            span_stack,
            message,
            fields: fields.0,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
                .pop(),
            Some(id.into_u64())
        );
    }
}

impl Trace {
    fn external_open_spans(&self) -> Vec<SpanRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured
            .spans
            .values()
            .filter(|span| span.target.starts_with("yard_shell") && span.name == "external-open")
            .cloned()
            .collect()
    }

    fn events_in_span(&self, span_id: u64) -> Vec<EventRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured
            .events
            .iter()
            .filter(|event| event.span_stack.contains(&span_id))
            .cloned()
            .collect()
    }
}

fn assert_span_field(span: &SpanRecord, name: &str, expected: &str) {
    assert_eq!(
        span.fields.get(name).map(String::as_str),
        Some(expected),
        "expected `{name}={expected}` on span {span:#?}"
    );
}

fn assert_no_span_field(span: &SpanRecord, name: &str) {
    assert!(
        !span.fields.contains_key(name),
        "did not expect `{name}` on span {span:#?}"
    );
}

fn assert_handed_off(events: &[EventRecord], expected_host: Option<&str>) {
    let handed_off: Vec<&EventRecord> = events
        .iter()
        .filter(|event| {
            event.level == tracing::Level::DEBUG && event.message.contains("handed off")
        })
        .collect();
    assert_eq!(
        handed_off.len(),
        1,
        "expected exactly one DEBUG `handed off` event, got {events:#?}"
    );
    assert_eq!(
        handed_off[0].fields.get("host").map(String::as_str),
        expected_host,
        "unexpected `host` on DEBUG `handed off` event: {:#?}",
        handed_off[0]
    );
    assert!(
        !events
            .iter()
            .any(|event| event.level == tracing::Level::INFO && event.message.contains("refused")),
        "accepted call must not log INFO `refused`: {events:#?}"
    );
}

fn assert_refused(events: &[EventRecord], expected_reason: &str) {
    assert!(
        events.iter().any(|event| {
            event.level == tracing::Level::INFO
                && event.message.contains("refused")
                && event.fields.get("reason").map(String::as_str) == Some(expected_reason)
        }),
        "expected INFO `refused` with reason={expected_reason:?}, got {events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| event.level == tracing::Level::DEBUG
                && event.message.contains("handed off")),
        "refused call must not log DEBUG `handed off`: {events:#?}"
    );
}

#[gpui::test]
fn external_open_logging_covers_accepted_and_refused_calls_without_leaking_inputs(
    cx: &mut TestAppContext,
) {
    INIT_LOGGER.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });

    let ext = cx.update(|cx| Extension::new(cx, "tour").unwrap());
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();

    let dir = tempfile::tempdir().unwrap();
    let private_dir = dir.path().join("yard-private-folder");
    fs::create_dir(&private_dir).unwrap();
    let blocked_script = private_dir.join("secret-probe.sh");
    fs::write(&blocked_script, "echo hi\n").unwrap();
    let missing_file = private_dir.join("missing-secret.txt");
    let invalid_relative_path = Path::new("relative-secret-dir/secret-file.txt");
    let over_length_scheme_url = format!("{}:overlong-payload", "q".repeat(MAX_URL_LEN + 16));
    let over_length_https_url = format!("https://overlong.example/{}", "z".repeat(MAX_URL_LEN));
    let ipv6_url = "http://[::1]:8080/";
    let expected_ipv6_host = url::Url::parse(ipv6_url).unwrap();
    assert_eq!(expected_ipv6_host.host_str(), Some("[::1]"));

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        // 1. Accepted https URL with path, query, and fragment.
        assert_eq!(
            cx.read(|cx| ext.open_url("https://example.com/secret-path?token=abc#frag", cx)),
            Ok(())
        );

        // 2. Refused disallowed scheme.
        assert_eq!(
            cx.read(|cx| ext.open_url("javascript:alert(1)", cx)),
            Err(ExternalOpenError::SchemeNotAllowed)
        );

        // 3. Refused https URL with userinfo.
        assert_eq!(
            cx.read(|cx| ext.open_url("https://user:pass@example.com/", cx)),
            Err(ExternalOpenError::InvalidUrl)
        );

        // 4. Refused mailto URL with `//` and address.
        assert_eq!(
            cx.read(|cx| ext.open_url("mailto://someone@example.com", cx)),
            Err(ExternalOpenError::InvalidUrl)
        );

        // 5. Refused empty mailto URL.
        assert_eq!(
            cx.read(|cx| ext.open_url("mailto:", cx)),
            Err(ExternalOpenError::InvalidUrl)
        );

        // 6. Accepted mailto URL (address must not appear in logs).
        assert_eq!(
            cx.read(|cx| ext.open_url("mailto:someone@example.com?subject=Hi", cx)),
            Ok(())
        );

        // 7. Refused open_path on a temporary file.
        assert_eq!(
            cx.read(|cx| ext.open_path(&blocked_script, cx)),
            Err(ExternalOpenError::Executable)
        );

        // 8. Refused reveal_path on a temporary file path.
        assert_eq!(
            cx.read(|cx| ext.reveal_path(&missing_file, cx)),
            Err(ExternalOpenError::NotFound)
        );

        // 9. Accepted mixed-case https URL (scheme lowercased, host on `handed off`).
        assert_eq!(
            cx.read(|cx| ext.open_url("HtTpS://Example.COM/", cx)),
            Ok(())
        );

        // 10. Refused unknown custom scheme (logged as `other`, never raw prefix).
        assert_eq!(
            cx.read(|cx| ext.open_url("secret-token:abc", cx)),
            Err(ExternalOpenError::SchemeNotAllowed)
        );

        // 11. Refused over-length input with an over-length scheme prefix.
        assert_eq!(
            cx.read(|cx| ext.open_url(&over_length_scheme_url, cx)),
            Err(ExternalOpenError::UrlTooLong)
        );

        // 12. Refused URL with no scheme (no `scheme` field).
        assert_eq!(
            cx.read(|cx| ext.open_url("example.com/no-scheme-secret", cx)),
            Err(ExternalOpenError::InvalidUrl)
        );

        // 13. Refused over-length https URL (no `scheme` field over 8192 bytes).
        assert_eq!(
            cx.read(|cx| ext.open_url(&over_length_https_url, cx)),
            Err(ExternalOpenError::UrlTooLong)
        );

        // 14. Refused open_path on a folder (`NotAFile`, no path in logs).
        assert_eq!(
            cx.read(|cx| ext.open_path(&private_dir, cx)),
            Err(ExternalOpenError::NotAFile)
        );

        // 15. Refused reveal_path on an invalid path (`InvalidPath`, no path in logs).
        assert_eq!(
            cx.read(|cx| ext.reveal_path(invalid_relative_path, cx)),
            Err(ExternalOpenError::InvalidPath)
        );

        // 16. Accepted http IPv6 URL (`host` logged as the parser gives it).
        assert_eq!(cx.read(|cx| ext.open_url(ipv6_url, cx)), Ok(()));
    });

    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    assert!(
        captured.entered.is_empty(),
        "every entered span exited cleanly"
    );
    drop(captured);

    let spans = trace.external_open_spans();
    assert_eq!(
        spans.len(),
        16,
        "expected 16 external-open spans: {spans:#?}"
    );
    for span in &spans {
        assert_eq!(
            span.level,
            tracing::Level::INFO,
            "external-open must be an info_span: {span:#?}"
        );
        assert_span_field(span, "extension", "tour");
        assert!(
            !span.declared_fields.iter().any(|field| field == "host"),
            "`host` must be on the `handed off` event, not on the external-open group: {span:#?}"
        );
        assert_no_span_field(span, "host");
    }

    // 1. Accepted https URL.
    let accepted_https = &spans[0];
    assert_span_field(accepted_https, "kind", "url");
    assert_span_field(accepted_https, "scheme", "https");
    assert_handed_off(
        &trace.events_in_span(accepted_https.id),
        Some("example.com"),
    );

    // 2. Refused javascript:alert(1).
    let refused_js = &spans[1];
    assert_span_field(refused_js, "kind", "url");
    assert_span_field(refused_js, "scheme", "javascript");
    assert_refused(&trace.events_in_span(refused_js.id), "scheme not allowed");

    // 3. Refused https://user:pass@example.com/.
    let refused_userinfo = &spans[2];
    assert_span_field(refused_userinfo, "kind", "url");
    assert_span_field(refused_userinfo, "scheme", "https");
    assert_refused(&trace.events_in_span(refused_userinfo.id), "invalid URL");

    // 4. Refused mailto://someone@example.com.
    let refused_mailto_slashes = &spans[3];
    assert_span_field(refused_mailto_slashes, "kind", "url");
    assert_span_field(refused_mailto_slashes, "scheme", "mailto");
    assert_refused(
        &trace.events_in_span(refused_mailto_slashes.id),
        "invalid URL",
    );

    // 5. Refused mailto:.
    let refused_mailto_empty = &spans[4];
    assert_span_field(refused_mailto_empty, "kind", "url");
    assert_span_field(refused_mailto_empty, "scheme", "mailto");
    assert_refused(
        &trace.events_in_span(refused_mailto_empty.id),
        "invalid URL",
    );

    // 6. Accepted mailto:someone@example.com?subject=Hi.
    let accepted_mailto = &spans[5];
    assert_span_field(accepted_mailto, "kind", "url");
    assert_span_field(accepted_mailto, "scheme", "mailto");
    assert_handed_off(&trace.events_in_span(accepted_mailto.id), None);

    // 7. Refused open_path.
    let refused_open = &spans[6];
    assert_span_field(refused_open, "kind", "open");
    assert_no_span_field(refused_open, "scheme");
    assert_refused(
        &trace.events_in_span(refused_open.id),
        "executable file refused",
    );

    // 8. Refused reveal_path.
    let refused_reveal = &spans[7];
    assert_span_field(refused_reveal, "kind", "reveal");
    assert_no_span_field(refused_reveal, "scheme");
    assert_refused(&trace.events_in_span(refused_reveal.id), "path not found");

    // 9. Accepted HtTpS://Example.COM/.
    let mixed_case_https = &spans[8];
    assert_span_field(mixed_case_https, "kind", "url");
    assert_span_field(mixed_case_https, "scheme", "https");
    assert_handed_off(
        &trace.events_in_span(mixed_case_https.id),
        Some("example.com"),
    );

    // 10. Refused secret-token:abc.
    let refused_secret_scheme = &spans[9];
    assert_span_field(refused_secret_scheme, "kind", "url");
    assert_span_field(refused_secret_scheme, "scheme", "other");
    assert_refused(
        &trace.events_in_span(refused_secret_scheme.id),
        "scheme not allowed",
    );

    // 11. Refused over-length input with an over-length scheme prefix (no `scheme` over 8192 bytes).
    let refused_over_length = &spans[10];
    assert_span_field(refused_over_length, "kind", "url");
    assert_no_span_field(refused_over_length, "scheme");
    assert_refused(
        &trace.events_in_span(refused_over_length.id),
        "URL too long",
    );

    // 12. Refused URL with no scheme.
    let refused_no_scheme = &spans[11];
    assert_span_field(refused_no_scheme, "kind", "url");
    assert_no_span_field(refused_no_scheme, "scheme");
    assert_refused(&trace.events_in_span(refused_no_scheme.id), "invalid URL");

    // 13. Refused over-length https URL (no `scheme` over 8192 bytes).
    let refused_over_length_https = &spans[12];
    assert_span_field(refused_over_length_https, "kind", "url");
    assert_no_span_field(refused_over_length_https, "scheme");
    assert_refused(
        &trace.events_in_span(refused_over_length_https.id),
        "URL too long",
    );

    // 14. Refused open_path on a folder (`NotAFile`).
    let refused_not_a_file = &spans[13];
    assert_span_field(refused_not_a_file, "kind", "open");
    assert_no_span_field(refused_not_a_file, "scheme");
    assert_refused(&trace.events_in_span(refused_not_a_file.id), "not a file");

    // 15. Refused reveal_path on an invalid path (`InvalidPath`).
    let refused_invalid_path = &spans[14];
    assert_span_field(refused_invalid_path, "kind", "reveal");
    assert_no_span_field(refused_invalid_path, "scheme");
    assert_refused(
        &trace.events_in_span(refused_invalid_path.id),
        "invalid path",
    );

    // 16. Accepted http://[::1]:8080/ (`host` logged as the parser gives it).
    let accepted_ipv6 = &spans[15];
    assert_span_field(accepted_ipv6, "kind", "url");
    assert_span_field(accepted_ipv6, "scheme", "http");
    assert_handed_off(
        &trace.events_in_span(accepted_ipv6.id),
        expected_ipv6_host.host_str(),
    );

    // No WARN or ERROR in tracing, and nothing written to the diagnostic log.
    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    let warnings_or_errors: Vec<&EventRecord> = captured
        .events
        .iter()
        .filter(|event| matches!(event.level, tracing::Level::WARN | tracing::Level::ERROR))
        .collect();
    assert!(
        warnings_or_errors.is_empty(),
        "expected no WARN or ERROR tracing events, got {warnings_or_errors:#?}"
    );

    let diagnostics = DIAGNOSTICS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    assert!(
        diagnostics.is_empty(),
        "external-open must not write to the diagnostic log, got {diagnostics:#?}"
    );

    // Privacy: no secret URL part, address, userinfo, file name, or temp path appears anywhere.
    let mut all_text = Vec::new();
    for span in captured.spans.values() {
        all_text.push(span.name.clone());
        for (key, value) in &span.fields {
            all_text.push(format!("{key}={value}"));
        }
    }
    for event in &captured.events {
        all_text.push(event.message.clone());
        for (key, value) in &event.fields {
            all_text.push(format!("{key}={value}"));
        }
    }
    for (_, message) in &diagnostics {
        all_text.push(message.clone());
    }

    let dir_path = dir.path().to_string_lossy().into_owned();
    let dir_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let private_dir_str = private_dir.to_string_lossy().into_owned();
    let blocked_script_str = blocked_script.to_string_lossy().into_owned();
    let missing_file_str = missing_file.to_string_lossy().into_owned();

    let over_length_needle = "q".repeat(32);
    let over_length_https_needle = "z".repeat(32);

    let forbidden = [
        "secret-path",
        "token=",
        "abc",
        "frag",
        "user",
        "pass",
        "someone@",
        "alert",
        "subject",
        "secret-token",
        "overlong-payload",
        "overlong.example",
        "no-scheme-secret",
        "relative-secret-dir",
        "secret-file.txt",
        "yard-private-folder",
        "secret-probe.sh",
        "secret-probe",
        "missing-secret.txt",
        "missing-secret",
        &over_length_needle,
        &over_length_https_needle,
        &dir_path,
        &dir_name,
        &private_dir_str,
        &blocked_script_str,
        &missing_file_str,
    ];

    for needle in forbidden {
        assert!(
            all_text.iter().all(|text| !text.contains(needle)),
            "log output must not contain `{needle}`: {all_text:#?}"
        );
    }
}
