// Alone in this binary: installs the process-wide diagnostic logger and
// captures tracing output under one scoped subscriber, so no other test can
// race on tracing's callsite interest cache.

use gpui::{PathPromptOptions, TestAppContext};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Once},
};

const CALLBACK_TARGET: &str = "file_dialogs_test_callback";

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
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct EventRecord {
    level: tracing::Level,
    target: String,
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
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let id = captured.spans.len() as u64 + 1;
        captured.spans.insert(
            id,
            SpanRecord {
                id,
                level: *attributes.metadata().level(),
                target: attributes.metadata().target().to_string(),
                name: attributes.metadata().name().to_string(),
                fields: fields.0,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = FieldMap::default();
        values.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(span) = captured.spans.get_mut(&id.into_u64()) {
            span.fields.extend(fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell") || target == CALLBACK_TARGET)
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.get("message").cloned().unwrap_or_default();
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let span_stack = captured.entered.clone();
        captured.events.push(EventRecord {
            level: *event.metadata().level(),
            target: target.to_string(),
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
    fn file_dialog_spans(&self) -> Vec<SpanRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured
            .spans
            .values()
            .filter(|span| span.target.starts_with("yard_shell") && span.name == "file-dialog")
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

fn is_yard_event(event: &EventRecord, level: tracing::Level, substring: &str) -> bool {
    event.target.starts_with("yard_shell")
        && event.level == level
        && event.message.contains(substring)
}

fn is_callback_event(event: &EventRecord, case: &str) -> bool {
    event.target == CALLBACK_TARGET && event.fields.get("case").map(String::as_str) == Some(case)
}

fn has_count(event: &EventRecord, expected: usize) -> bool {
    let expected = expected.to_string();
    event
        .fields
        .iter()
        .any(|(name, value)| name != "message" && value == &expected)
        || event
            .message
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .any(|token| token == expected)
}

fn assert_span_field(span: &SpanRecord, name: &str, expected: &str) {
    assert_eq!(
        span.fields.get(name).map(String::as_str),
        Some(expected),
        "expected `{name}={expected}` on span {span:#?}"
    );
}

#[gpui::test]
fn file_dialogs_log_groups_and_outcomes_at_the_expected_levels(cx: &mut TestAppContext) {
    INIT_LOGGER.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let ext = cx.update(|cx| yard_shell::Extension::new(cx, "tour").unwrap());

        // 1. Open dialog choosing two files.
        cx.update(|cx| {
            ext.choose_paths(
                PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some("Import".into()),
                },
                cx,
                |_, _| {
                    tracing::debug!(
                        target: CALLBACK_TARGET,
                        case = "open-chosen",
                        "the test callback ran"
                    );
                },
            );
        });
        assert!(cx.did_prompt_for_paths());
        cx.run_until_parked();
        let open_chosen_span = trace
            .file_dialog_spans()
            .last()
            .cloned()
            .expect("open-chosen opened a file-dialog group");
        let before_answer = trace.events_in_span(open_chosen_span.id);
        assert!(
            before_answer
                .iter()
                .any(|event| is_yard_event(event, tracing::Level::DEBUG, "shown")),
            "`shown` is logged while the open dialog is pending: {before_answer:#?}"
        );
        assert!(
            !before_answer.iter().any(|event| is_yard_event(
                event,
                tracing::Level::DEBUG,
                "chosen"
            )),
            "`chosen` is not logged before the answer: {before_answer:#?}"
        );
        cx.simulate_path_prompt_response(|_| {
            Some(vec![
                PathBuf::from("/tmp/alpha.txt"),
                PathBuf::from("/tmp/beta.txt"),
            ])
        });
        cx.run_until_parked();

        // 2. Open dialog cancelled.
        cx.update(|cx| {
            ext.choose_paths(
                PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: None,
                },
                cx,
                |_, _| {
                    tracing::debug!(
                        target: CALLBACK_TARGET,
                        case = "open-cancelled",
                        "the test callback ran"
                    );
                },
            );
        });
        assert!(cx.did_prompt_for_paths());
        cx.run_until_parked();
        let open_cancelled_span = trace
            .file_dialog_spans()
            .last()
            .cloned()
            .expect("open-cancelled opened a file-dialog group");
        assert!(
            trace
                .events_in_span(open_cancelled_span.id)
                .iter()
                .any(|event| is_yard_event(event, tracing::Level::DEBUG, "shown")),
            "`shown` is logged before cancelling the open dialog"
        );
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();

        // 3. Open dialog refused because files and directories are both false.
        cx.update(|cx| {
            ext.choose_paths(
                PathPromptOptions {
                    files: false,
                    directories: false,
                    multiple: false,
                    prompt: None,
                },
                cx,
                |_, _| {
                    tracing::debug!(
                        target: CALLBACK_TARGET,
                        case = "open-refused",
                        "the test callback ran"
                    );
                },
            );
        });
        assert!(!cx.did_prompt_for_paths());
        cx.run_until_parked();

        // 4. Save dialog choosing one path.
        cx.update(|cx| {
            ext.choose_new_path(Path::new("/tmp/exports"), Some("report.txt"), cx, |_, _| {
                tracing::debug!(
                    target: CALLBACK_TARGET,
                    case = "save-chosen",
                    "the test callback ran"
                );
            });
        });
        assert!(cx.did_prompt_for_new_path());
        cx.run_until_parked();
        let save_chosen_span = trace
            .file_dialog_spans()
            .last()
            .cloned()
            .expect("save-chosen opened a file-dialog group");
        let before_answer = trace.events_in_span(save_chosen_span.id);
        assert!(
            before_answer
                .iter()
                .any(|event| is_yard_event(event, tracing::Level::DEBUG, "shown")),
            "`shown` is logged while the save dialog is pending: {before_answer:#?}"
        );
        assert!(
            !before_answer.iter().any(|event| is_yard_event(
                event,
                tracing::Level::DEBUG,
                "chosen"
            )),
            "`chosen` is not logged before the save answer: {before_answer:#?}"
        );
        cx.simulate_new_path_selection(|_| Some(PathBuf::from("/tmp/exports/output.txt")));
        cx.run_until_parked();

        // 5. Save dialog cancelled.
        cx.update(|cx| {
            ext.choose_new_path(Path::new("/tmp/exports"), None, cx, |_, _| {
                tracing::debug!(
                    target: CALLBACK_TARGET,
                    case = "save-cancelled",
                    "the test callback ran"
                );
            });
        });
        assert!(cx.did_prompt_for_new_path());
        cx.run_until_parked();
        let save_cancelled_span = trace
            .file_dialog_spans()
            .last()
            .cloned()
            .expect("save-cancelled opened a file-dialog group");
        assert!(
            trace
                .events_in_span(save_cancelled_span.id)
                .iter()
                .any(|event| is_yard_event(event, tracing::Level::DEBUG, "shown")),
            "`shown` is logged before cancelling the save dialog"
        );
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    });

    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    assert!(
        captured.entered.is_empty(),
        "every entered span exited cleanly"
    );
    drop(captured);

    let spans = trace.file_dialog_spans();
    assert_eq!(spans.len(), 5, "five file-dialog groups in {spans:#?}");
    for span in &spans {
        assert_eq!(
            span.level,
            tracing::Level::INFO,
            "file-dialog is an info_span: {span:#?}"
        );
        assert_span_field(span, "extension", "tour");
    }

    // 1. Open chosen (2 paths).
    let open_chosen = &spans[0];
    assert_span_field(open_chosen, "kind", "open");
    assert_span_field(open_chosen, "files", "true");
    assert_span_field(open_chosen, "directories", "false");
    assert_span_field(open_chosen, "multiple", "true");
    let events = trace.events_in_span(open_chosen.id);
    let shown = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::DEBUG, "shown"))
        .unwrap_or_else(|| panic!("expected DEBUG `shown` in open-chosen: {events:#?}"));
    let chosen = events
        .iter()
        .position(|event| {
            is_yard_event(event, tracing::Level::DEBUG, "chosen") && has_count(event, 2)
        })
        .unwrap_or_else(|| {
            panic!("expected DEBUG `chosen` with count 2 in open-chosen: {events:#?}")
        });
    let callback = events
        .iter()
        .position(|event| is_callback_event(event, "open-chosen"))
        .unwrap_or_else(|| panic!("expected callback inside open-chosen group: {events:#?}"));
    assert!(
        shown < chosen && chosen < callback,
        "expected shown < chosen < callback in open-chosen: {events:#?}"
    );

    // 2. Open cancelled.
    let open_cancelled = &spans[1];
    assert_span_field(open_cancelled, "kind", "open");
    assert_span_field(open_cancelled, "files", "false");
    assert_span_field(open_cancelled, "directories", "true");
    assert_span_field(open_cancelled, "multiple", "false");
    let events = trace.events_in_span(open_cancelled.id);
    let shown = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::DEBUG, "shown"))
        .unwrap_or_else(|| panic!("expected DEBUG `shown` in open-cancelled: {events:#?}"));
    let cancelled = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::INFO, "cancelled"))
        .unwrap_or_else(|| panic!("expected INFO `cancelled` in open-cancelled: {events:#?}"));
    let callback = events
        .iter()
        .position(|event| is_callback_event(event, "open-cancelled"))
        .unwrap_or_else(|| panic!("expected callback inside open-cancelled group: {events:#?}"));
    assert!(
        !events
            .iter()
            .any(|event| is_yard_event(event, tracing::Level::DEBUG, "chosen")),
        "cancelled open dialog must not log `chosen`: {events:#?}"
    );
    assert!(
        shown < cancelled && cancelled < callback,
        "expected shown < cancelled < callback in open-cancelled: {events:#?}"
    );

    // 3. Open refused (files=false, directories=false).
    let open_refused = &spans[2];
    assert_span_field(open_refused, "kind", "open");
    assert_span_field(open_refused, "files", "false");
    assert_span_field(open_refused, "directories", "false");
    assert_span_field(open_refused, "multiple", "false");
    let events = trace.events_in_span(open_refused.id);
    assert!(
        !events
            .iter()
            .any(|event| is_yard_event(event, tracing::Level::DEBUG, "shown")),
        "refused open dialog must not log DEBUG `shown`: {events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| is_yard_event(event, tracing::Level::DEBUG, "chosen")),
        "refused open dialog must not log DEBUG `chosen`: {events:#?}"
    );
    let refused = events
        .iter()
        .position(|event| {
            event.target.starts_with("yard_shell")
                && event.level == tracing::Level::INFO
                && !event.message.is_empty()
        })
        .unwrap_or_else(|| panic!("expected INFO refusal line in open-refused: {events:#?}"));
    let callback = events
        .iter()
        .position(|event| is_callback_event(event, "open-refused"))
        .unwrap_or_else(|| panic!("expected callback inside open-refused group: {events:#?}"));
    assert!(
        refused < callback,
        "expected refusal INFO before callback in open-refused: {events:#?}"
    );

    // 4. Save chosen (1 path).
    let save_chosen = &spans[3];
    assert_span_field(save_chosen, "kind", "save");
    let events = trace.events_in_span(save_chosen.id);
    let shown = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::DEBUG, "shown"))
        .unwrap_or_else(|| panic!("expected DEBUG `shown` in save-chosen: {events:#?}"));
    let chosen = events
        .iter()
        .position(|event| {
            is_yard_event(event, tracing::Level::DEBUG, "chosen") && has_count(event, 1)
        })
        .unwrap_or_else(|| {
            panic!("expected DEBUG `chosen` with count 1 in save-chosen: {events:#?}")
        });
    let callback = events
        .iter()
        .position(|event| is_callback_event(event, "save-chosen"))
        .unwrap_or_else(|| panic!("expected callback inside save-chosen group: {events:#?}"));
    assert!(
        shown < chosen && chosen < callback,
        "expected shown < chosen < callback in save-chosen: {events:#?}"
    );

    // 5. Save cancelled.
    let save_cancelled = &spans[4];
    assert_span_field(save_cancelled, "kind", "save");
    let events = trace.events_in_span(save_cancelled.id);
    let shown = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::DEBUG, "shown"))
        .unwrap_or_else(|| panic!("expected DEBUG `shown` in save-cancelled: {events:#?}"));
    let cancelled = events
        .iter()
        .position(|event| is_yard_event(event, tracing::Level::INFO, "cancelled"))
        .unwrap_or_else(|| panic!("expected INFO `cancelled` in save-cancelled: {events:#?}"));
    let callback = events
        .iter()
        .position(|event| is_callback_event(event, "save-cancelled"))
        .unwrap_or_else(|| panic!("expected callback inside save-cancelled group: {events:#?}"));
    assert!(
        !events
            .iter()
            .any(|event| is_yard_event(event, tracing::Level::DEBUG, "chosen")),
        "cancelled save dialog must not log `chosen`: {events:#?}"
    );
    assert!(
        shown < cancelled && cancelled < callback,
        "expected shown < cancelled < callback in save-cancelled: {events:#?}"
    );

    // No WARN or ERROR anywhere in the tracing events or diagnostic log.
    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    let warnings_or_errors: Vec<&EventRecord> = captured
        .events
        .iter()
        .filter(|event| {
            event.target.starts_with("yard_shell")
                && matches!(event.level, tracing::Level::WARN | tracing::Level::ERROR)
        })
        .collect();
    assert!(
        warnings_or_errors.is_empty(),
        "expected no WARN or ERROR tracing lines, got: {warnings_or_errors:#?}"
    );

    let diagnostics = DIAGNOSTICS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    assert!(
        diagnostics
            .iter()
            .all(|(level, _)| !matches!(level, log::Level::Warn | log::Level::Error)),
        "expected no WARN or ERROR diagnostic records, got: {diagnostics:#?}"
    );
    for path in ["/tmp/alpha.txt", "/tmp/beta.txt", "/tmp/exports/output.txt"] {
        assert!(
            diagnostics
                .iter()
                .all(|(_, message)| !message.contains(path)),
            "diagnostic log must never hold a chosen path ({path}): {diagnostics:#?}"
        );
    }
}
