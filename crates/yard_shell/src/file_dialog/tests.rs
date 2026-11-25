use super::finish_dialog;
use futures::channel::oneshot;
use gpui::TestAppContext;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

const CALLBACK_TARGET: &str = "file_dialog_unit_test_callback";

// Every test in this module runs under one lock and one scoped subscriber so
// no thread ever hits `finish_dialog`'s tracing callsites without a subscriber
// (no other unit test in `yard_shell` calls `file_dialog`).
//
// `log::set_logger` is intentionally not installed here: `AGENTS.md` reserves
// process-wide `log` loggers for standalone integration test binaries.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    name: String,
}

#[derive(Clone, Debug)]
struct EventRecord {
    level: tracing::Level,
    target: String,
    span_stack: Vec<u64>,
    message: String,
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
struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}").trim_matches('"').to_string();
        }
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
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let id = captured.spans.len() as u64 + 1;
        captured.spans.insert(
            id,
            SpanRecord {
                id,
                name: attributes.metadata().name().to_string(),
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell") || target == CALLBACK_TARGET)
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let span_stack = captured.entered.clone();
        captured.events.push(EventRecord {
            level: *event.metadata().level(),
            target: target.to_string(),
            span_stack,
            message: visitor.0,
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
    fn events_in_file_dialog(&self) -> Vec<EventRecord> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let span_id = captured
            .spans
            .values()
            .find(|span| span.name == "file-dialog")
            .expect("a file-dialog span was created")
            .id;
        captured
            .events
            .iter()
            .filter(|event| event.span_stack.contains(&span_id))
            .cloned()
            .collect()
    }
}

fn with_trace(run: impl FnOnce(&Trace)) {
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || run(&trace));
    assert!(
        trace
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .is_empty(),
        "every entered span exited cleanly"
    );
}

#[gpui::test]
fn finish_dialog_delivers_none_when_the_dialog_fails(cx: &mut TestAppContext) {
    with_trace(|trace| {
        let (tx, rx) = oneshot::channel::<gpui::Result<Option<Vec<PathBuf>>>>();
        let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        cx.update(|cx| {
            let span = tracing::info_span!(
                "file-dialog",
                extension = "tour",
                kind = "open",
                files = true,
                directories = false,
                multiple = false,
            );
            finish_dialog(rx, span, Vec::as_slice, cx, move |paths, _| {
                tracing::debug!(target: CALLBACK_TARGET, "callback ran");
                sink.borrow_mut().push(paths);
            });
        });

        cx.run_until_parked();
        assert!(
            received.borrow().is_empty(),
            "the callback must not run before the dialog finishes"
        );

        tx.send(Err(std::io::Error::other("no portal").into()))
            .expect("receiver is waiting");
        cx.run_until_parked();

        assert_eq!(
            *received.borrow(),
            vec![None],
            "a platform error delivers None once"
        );
        cx.run_until_parked();
        assert_eq!(received.borrow().len(), 1, "the callback runs exactly once");

        let events = trace.events_in_file_dialog();
        assert!(
            events.iter().any(|event| event.target == CALLBACK_TARGET),
            "the callback runs inside the file-dialog group: {events:#?}"
        );
    });
}

#[gpui::test]
fn finish_dialog_delivers_none_and_logs_info_when_the_sender_drops(cx: &mut TestAppContext) {
    with_trace(|trace| {
        let (tx, rx) = oneshot::channel::<gpui::Result<Option<Vec<PathBuf>>>>();
        let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        cx.update(|cx| {
            let span = tracing::info_span!(
                "file-dialog",
                extension = "tour",
                kind = "open",
                files = true,
                directories = false,
                multiple = false,
            );
            finish_dialog(rx, span, Vec::as_slice, cx, move |paths, _| {
                tracing::debug!(target: CALLBACK_TARGET, "callback ran");
                sink.borrow_mut().push(paths);
            });
        });

        cx.run_until_parked();
        assert!(
            received.borrow().is_empty(),
            "the callback must not run while the sender is still alive"
        );

        drop(tx);
        cx.run_until_parked();

        assert_eq!(
            *received.borrow(),
            vec![None],
            "a dropped sender delivers None once"
        );
        cx.run_until_parked();
        assert_eq!(received.borrow().len(), 1, "the callback runs exactly once");

        let events = trace.events_in_file_dialog();
        let info = events
            .iter()
            .position(|event| {
                event.target.starts_with("yard_shell")
                    && event.level == tracing::Level::INFO
                    && !event.message.is_empty()
            })
            .unwrap_or_else(|| {
                panic!("expected an INFO line inside the file-dialog group: {events:#?}")
            });
        let callback = events
            .iter()
            .position(|event| event.target == CALLBACK_TARGET)
            .unwrap_or_else(|| {
                panic!("expected callback inside the file-dialog group: {events:#?}")
            });
        assert!(
            info < callback,
            "expected INFO line before callback inside the file-dialog group: {events:#?}"
        );
    });
}

#[gpui::test]
fn finish_dialog_delivers_an_empty_selection_unchanged(cx: &mut TestAppContext) {
    with_trace(|trace| {
        let (tx, rx) = oneshot::channel::<gpui::Result<Option<Vec<PathBuf>>>>();
        let received: Rc<RefCell<Vec<Option<Vec<PathBuf>>>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = received.clone();

        cx.update(|cx| {
            let span = tracing::info_span!(
                "file-dialog",
                extension = "tour",
                kind = "open",
                files = true,
                directories = false,
                multiple = true,
            );
            finish_dialog(rx, span, Vec::as_slice, cx, move |paths, _| {
                tracing::debug!(target: CALLBACK_TARGET, "callback ran");
                sink.borrow_mut().push(paths);
            });
        });

        tx.send(Ok(Some(Vec::new()))).expect("receiver is waiting");
        cx.run_until_parked();

        assert_eq!(
            *received.borrow(),
            vec![Some(Vec::new())],
            "Ok(Some(vec![])) reaches the callback unchanged, once"
        );
        cx.run_until_parked();
        assert_eq!(received.borrow().len(), 1, "the callback runs exactly once");

        let events = trace.events_in_file_dialog();
        assert!(
            events.iter().any(|event| event.target == CALLBACK_TARGET),
            "the callback runs inside the file-dialog group: {events:#?}"
        );
    });
}
