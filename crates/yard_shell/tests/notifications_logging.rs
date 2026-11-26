#![cfg(feature = "dev")]

// Every test in this binary takes `LOCK` and runs under one scoped tracing
// subscriber (with the process-wide diagnostic logger bridging `log` records
// into it, as `yard_log` does in production), so no test can race on tracing's
// callsite interest cache.

use gpui::{
    App, SystemNotification, SystemNotificationAction, SystemNotificationResponse, TestAppContext,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Once},
};

const HANDLER_TARGET: &str = "notifications_test_handler";
const DIAGNOSTIC_TARGET: &str = "notifications_test_diagnostic";

static INIT_LOGGER: Once = Once::new();
static LOCK: Mutex<()> = Mutex::new(());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            log::Level::Error => tracing::error!(target: DIAGNOSTIC_TARGET, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: DIAGNOSTIC_TARGET, "{}", record.args()),
            log::Level::Info => tracing::info!(target: DIAGNOSTIC_TARGET, "{}", record.args()),
            log::Level::Debug => tracing::debug!(target: DIAGNOSTIC_TARGET, "{}", record.args()),
            log::Level::Trace => tracing::trace!(target: DIAGNOSTIC_TARGET, "{}", record.args()),
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
    spans: Vec<SpanRecord>,
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
        captured.spans.push(SpanRecord {
            id,
            level: *attributes.metadata().level(),
            target: attributes.metadata().target().to_string(),
            name: attributes.metadata().name().to_string(),
            fields: fields.0,
        });
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = FieldMap::default();
        values.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(span) = captured
            .spans
            .iter_mut()
            .find(|span| span.id == id.into_u64())
        {
            span.fields.extend(fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell")
            || target == HANDLER_TARGET
            || target == DIAGNOSTIC_TARGET)
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
    fn span_mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .len()
    }

    fn event_mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .len()
    }

    fn spans_named_since(&self, mark: usize, name: &str) -> Vec<SpanRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured.spans[mark..]
            .iter()
            .filter(|span| span.target.starts_with("yard_shell") && span.name == name)
            .cloned()
            .collect()
    }

    fn events_since(&self, mark: usize) -> Vec<EventRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured.events[mark..].to_vec()
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

fn is_shell_event(event: &EventRecord, level: tracing::Level, substring: &str) -> bool {
    event.target.starts_with("yard_shell")
        && event.level == level
        && event.message.contains(substring)
}

fn is_shell_or_diagnostic(event: &EventRecord) -> bool {
    event.target.starts_with("yard_shell") || event.target == DIAGNOSTIC_TARGET
}

fn assert_span_field(span: &SpanRecord, name: &str, expected: &str) {
    assert_eq!(
        span.fields.get(name).map(String::as_str),
        Some(expected),
        "expected `{name}={expected}` on span {span:#?}"
    );
}

fn assert_no_warn_or_error(events: &[EventRecord], context: &str) {
    let bad: Vec<&EventRecord> = events
        .iter()
        .filter(|event| {
            is_shell_or_diagnostic(event)
                && matches!(event.level, tracing::Level::WARN | tracing::Level::ERROR)
        })
        .collect();
    assert!(
        bad.is_empty(),
        "expected no WARN or ERROR during {context}, got: {bad:#?}"
    );
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        INIT_LOGGER.call_once(|| {
            log::set_logger(&Diagnostics).unwrap();
            log::set_max_level(log::LevelFilter::Trace);
        });
        test(cx, &trace);
    });
    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    assert!(
        captured.entered.is_empty(),
        "every entered span must exit cleanly"
    );
}

#[gpui::test]
fn notifications_log_groups_fields_and_levels(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        cx.update(|cx| cx.set_app_identity("dev.yard.test", "Yard Test"));
        let alpha = cx.update(|cx| yard_shell::Extension::new(cx, "alpha").unwrap());
        let _quiet = cx.update(|cx| yard_shell::Extension::new(cx, "quiet").unwrap());

        // 1. First handler registration: `notification-handler-register{extension}` with DEBUG `registered`.
        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.update(|cx| {
            alpha.on_notification_response(
                cx,
                |response: SystemNotificationResponse, _: &mut App| {
                    tracing::debug!(
                        target: HANDLER_TARGET,
                        tag = response.tag.as_ref(),
                        "test notification handler ran"
                    );
                },
            );
        });
        let spans = trace.spans_named_since(span_mark, "notification-handler-register");
        assert_eq!(
            spans.len(),
            1,
            "first registration must open one `notification-handler-register` group: {spans:#?}"
        );
        let register_span = &spans[0];
        assert_eq!(register_span.level, tracing::Level::INFO);
        assert_span_field(register_span, "extension", "alpha");
        let events = trace.events_in_span(register_span.id);
        assert!(
            events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "registered")),
            "expected DEBUG `registered` in notification-handler-register: {events:#?}"
        );
        assert_no_warn_or_error(&trace.events_since(event_mark), "first registration");

        // 2. Post notification: `notification{extension, tag, kind="show"}` with DEBUG `posted` carrying `title` and `actions`.
        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.update(|cx| {
            alpha.show_notification(
                SystemNotification {
                    tag: "build".into(),
                    title: "Build finished".into(),
                    body: "3 warnings".into(),
                    actions: vec![
                        SystemNotificationAction {
                            id: "open".into(),
                            label: "Open".into(),
                        },
                        SystemNotificationAction {
                            id: "mute".into(),
                            label: "Mute".into(),
                        },
                    ],
                },
                cx,
            );
        });
        let spans = trace.spans_named_since(span_mark, "notification");
        assert_eq!(
            spans.len(),
            1,
            "show_notification must open one `notification` group: {spans:#?}"
        );
        let show_span = &spans[0];
        assert_eq!(show_span.level, tracing::Level::INFO);
        assert_span_field(show_span, "extension", "alpha");
        assert_span_field(show_span, "tag", "build");
        assert_span_field(show_span, "kind", "show");
        let events = trace.events_in_span(show_span.id);
        let posted = events
            .iter()
            .find(|event| is_shell_event(event, tracing::Level::DEBUG, "posted"))
            .unwrap_or_else(|| {
                panic!("expected DEBUG `posted` in notification show group: {events:#?}")
            });
        assert_eq!(
            posted.fields.get("title").map(String::as_str),
            Some("Build finished"),
            "expected `title=\"Build finished\"` on `posted` event: {posted:#?}"
        );
        assert_eq!(
            posted.fields.get("actions").map(String::as_str),
            Some("2"),
            "expected `actions=2` on `posted` event: {posted:#?}"
        );
        assert_no_warn_or_error(&trace.events_since(event_mark), "show_notification");

        // 3. Dismiss notification: `notification{extension, tag, kind="dismiss"}` with DEBUG `dismissed`.
        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.update(|cx| {
            alpha.dismiss_notification("build", cx);
        });
        let spans = trace.spans_named_since(span_mark, "notification");
        assert_eq!(
            spans.len(),
            1,
            "dismiss_notification must open one `notification` group: {spans:#?}"
        );
        let dismiss_span = &spans[0];
        assert_eq!(dismiss_span.level, tracing::Level::INFO);
        assert_span_field(dismiss_span, "extension", "alpha");
        assert_span_field(dismiss_span, "tag", "build");
        assert_span_field(dismiss_span, "kind", "dismiss");
        let events = trace.events_in_span(dismiss_span.id);
        assert!(
            events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "dismissed")),
            "expected DEBUG `dismissed` in notification dismiss group: {events:#?}"
        );
        assert_no_warn_or_error(&trace.events_since(event_mark), "dismiss_notification");

        // 4. Handled body response: `notification-response{extension, tag, action=""}` with DEBUG `handled` and handler line inside.
        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/build".into(),
            action_id: None,
        });
        cx.run_until_parked();
        let spans = trace.spans_named_since(span_mark, "notification-response");
        assert_eq!(
            spans.len(),
            1,
            "handled body response must open one `notification-response` group: {spans:#?}"
        );
        let body_span = &spans[0];
        assert_eq!(body_span.level, tracing::Level::INFO);
        assert_span_field(body_span, "extension", "alpha");
        assert_span_field(body_span, "tag", "build");
        assert_span_field(body_span, "action", "");
        let events = trace.events_in_span(body_span.id);
        assert!(
            events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "handled")),
            "expected DEBUG `handled` in body notification-response group: {events:#?}"
        );
        assert!(
            events.iter().any(|event| {
                event.target == HANDLER_TARGET
                    && event.fields.get("tag").map(String::as_str) == Some("build")
            }),
            "expected handler's own log line inside body notification-response group: {events:#?}"
        );
        assert_no_warn_or_error(&trace.events_since(event_mark), "handled body response");

        // 5. Handled action response: `notification-response{extension, tag, action="open"}` with DEBUG `handled` and handler line inside.
        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/build".into(),
            action_id: Some("open".into()),
        });
        cx.run_until_parked();
        let spans = trace.spans_named_since(span_mark, "notification-response");
        assert_eq!(
            spans.len(),
            1,
            "handled action response must open one `notification-response` group: {spans:#?}"
        );
        let action_span = &spans[0];
        assert_eq!(action_span.level, tracing::Level::INFO);
        assert_span_field(action_span, "extension", "alpha");
        assert_span_field(action_span, "tag", "build");
        assert_span_field(action_span, "action", "open");
        let events = trace.events_in_span(action_span.id);
        assert!(
            events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "handled")),
            "expected DEBUG `handled` in action notification-response group: {events:#?}"
        );
        assert!(
            events.iter().any(|event| {
                event.target == HANDLER_TARGET
                    && event.fields.get("tag").map(String::as_str) == Some("build")
            }),
            "expected handler's own log line inside action notification-response group: {events:#?}"
        );
        assert_no_warn_or_error(&trace.events_since(event_mark), "handled action response");

        // 6. Unhandled response (extension with no handler) and unknown extension tags:
        //    each opens `notification-response{extension, tag, action}` with INFO inside it and no WARN/ERROR.
        for (label, response, expected_ext, expected_tag, expected_action) in [
            (
                "extension without handler",
                SystemNotificationResponse {
                    tag: "quiet/ping".into(),
                    action_id: None,
                },
                "quiet",
                "ping",
                "",
            ),
            (
                "unknown extension prefix",
                SystemNotificationResponse {
                    tag: "ghost/x".into(),
                    action_id: None,
                },
                "ghost",
                "x",
                "",
            ),
            (
                "tag without slash",
                SystemNotificationResponse {
                    tag: "orphan".into(),
                    action_id: Some("open".into()),
                },
                "",
                "orphan",
                "open",
            ),
        ] {
            let span_mark = trace.span_mark();
            let event_mark = trace.event_mark();
            cx.simulate_system_notification_response(response);
            cx.run_until_parked();
            let spans = trace.spans_named_since(span_mark, "notification-response");
            assert_eq!(
                spans.len(),
                1,
                "{label} must open one `notification-response` group: {spans:#?}"
            );
            let unhandled_span = &spans[0];
            assert_eq!(unhandled_span.level, tracing::Level::INFO);
            assert_span_field(unhandled_span, "extension", expected_ext);
            assert_span_field(unhandled_span, "tag", expected_tag);
            assert_span_field(unhandled_span, "action", expected_action);
            let in_group = trace.events_in_span(unhandled_span.id);
            assert!(
                in_group.iter().any(|event| {
                    event.target.starts_with("yard_shell") && event.level == tracing::Level::INFO
                }),
                "expected INFO line inside `notification-response` for {label}, got: {in_group:#?}"
            );
            let events = trace.events_since(event_mark);
            assert!(
                !events
                    .iter()
                    .any(|event| is_shell_event(event, tracing::Level::DEBUG, "handled")),
                "{label} must not log DEBUG `handled`: {events:#?}"
            );
            assert!(
                !events.iter().any(|event| event.target == HANDLER_TARGET),
                "{label} must not invoke a handler: {events:#?}"
            );
            assert_no_warn_or_error(&events, label);
        }

        // 7. Second registration for the same extension: `notification-handler-register{extension}` with ERROR.
        let span_mark = trace.span_mark();
        cx.update(|cx| {
            alpha.on_notification_response(cx, |_: SystemNotificationResponse, _: &mut App| {});
        });
        let spans = trace.spans_named_since(span_mark, "notification-handler-register");
        assert_eq!(
            spans.len(),
            1,
            "second registration must open one `notification-handler-register` group: {spans:#?}"
        );
        let second_span = &spans[0];
        assert_eq!(second_span.level, tracing::Level::INFO);
        assert_span_field(second_span, "extension", "alpha");
        let events = trace.events_in_span(second_span.id);
        assert!(
            events.iter().any(|event| {
                is_shell_or_diagnostic(event) && event.level == tracing::Level::ERROR
            }),
            "expected ERROR in second notification-handler-register group: {events:#?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "registered")),
            "refused second registration must not log DEBUG `registered`: {events:#?}"
        );
    });
}

#[gpui::test]
fn first_show_installs_response_handler_and_logs_info_in_group(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        cx.update(|cx| cx.set_app_identity("dev.yard.test", "Yard Test"));
        let alpha = cx.update(|cx| yard_shell::Extension::new(cx, "alpha").unwrap());

        cx.update(|cx| {
            alpha.show_notification(
                SystemNotification {
                    tag: "ping".into(),
                    title: "Ping".into(),
                    body: "No handler registered".into(),
                    actions: Vec::new(),
                },
                cx,
            );
        });

        let span_mark = trace.span_mark();
        let event_mark = trace.event_mark();
        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/ping".into(),
            action_id: None,
        });
        cx.run_until_parked();

        let spans = trace.spans_named_since(span_mark, "notification-response");
        assert_eq!(
            spans.len(),
            1,
            "response after show_notification with no registered handler must open one `notification-response` group: {spans:#?}"
        );
        let response_span = &spans[0];
        assert_eq!(response_span.level, tracing::Level::INFO);
        assert_span_field(response_span, "extension", "alpha");
        assert_span_field(response_span, "tag", "ping");
        assert_span_field(response_span, "action", "");
        let in_group = trace.events_in_span(response_span.id);
        assert!(
            in_group.iter().any(|event| {
                event.target.starts_with("yard_shell") && event.level == tracing::Level::INFO
            }),
            "expected INFO line inside `notification-response` when no handler was registered: {in_group:#?}"
        );
        let events = trace.events_since(event_mark);
        assert!(
            !events
                .iter()
                .any(|event| is_shell_event(event, tracing::Level::DEBUG, "handled")),
            "unhandled response must not log DEBUG `handled`: {events:#?}"
        );
        assert_no_warn_or_error(&events, "response after show without handler");
    });
}
