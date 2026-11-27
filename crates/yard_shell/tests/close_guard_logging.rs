#![cfg(feature = "dev")]

use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, PromptLevel, TestAppContext, VisualTestContext, WindowHandle, WindowOptions,
    px, size,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use yard_shell::{
    CloseDecision, CloseOutcome, Extension, Prompt, Quit, Shell, bind_default_keys, handle_quit,
    new_shell,
};

// Re-emit diagnostic `log` records as tracing events under this target so one
// subscriber captures both logs with their enclosing groups.
const PROBE: &str = "close_guard_probe";

static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        RECORDS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((record.level(), record.args().to_string()));
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            log::Level::Info => tracing::info!(target: PROBE, "{}", record.args()),
            log::Level::Debug => tracing::debug!(target: PROBE, "{}", record.args()),
            log::Level::Trace => tracing::trace!(target: PROBE, "{}", record.args()),
        }
    }

    fn flush(&self) {}
}

fn errors() -> Vec<String> {
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message.clone())
        .collect()
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    // Each created `yard_shell` span with the stack of enclosing span IDs.
    span_stacks: Vec<Vec<u64>>,
    // Each event from `yard_shell` or `PROBE` (including `TRACE`, needed for
    // `window-edited`): `(enclosing span IDs, level, fields)`.
    events: Vec<(Vec<u64>, tracing::Level, String)>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        captured.spans.insert(id, label);
        if attributes.metadata().target().starts_with("yard_shell") {
            let mut stack = captured.entered.clone();
            stack.push(id);
            captured.span_stacks.push(stack);
        }
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if fields.0.is_empty() {
            return;
        }
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(span) = captured.spans.get_mut(&id.into_u64()) {
            span.push_str(&fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell") || target == PROBE) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let stack = captured.entered.clone();
        captured
            .events
            .push((stack, *event.metadata().level(), fields.0));
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

#[derive(Clone, Debug)]
struct RecordedEvent {
    stack: Vec<String>,
    level: tracing::Level,
    fields: String,
}

impl RecordedEvent {
    fn group(&self) -> &str {
        self.stack.last().map(String::as_str).unwrap_or("none")
    }
}

impl Trace {
    fn event_mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .events
            .len()
    }

    fn stack_mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .span_stacks
            .len()
    }

    fn events_since(&self, mark: usize) -> Vec<RecordedEvent> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured.events[mark..]
            .iter()
            .map(|(stack, level, fields)| RecordedEvent {
                stack: stack
                    .iter()
                    .map(|id| captured.spans[id].replace('"', ""))
                    .collect(),
                level: *level,
                fields: fields.replace('"', ""),
            })
            .collect()
    }

    fn stacks_since(&self, mark: usize) -> Vec<Vec<String>> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured.span_stacks[mark..]
            .iter()
            .map(|stack| {
                stack
                    .iter()
                    .map(|id| captured.spans[id].replace('"', ""))
                    .collect()
            })
            .collect()
    }
}

fn open_window(app: &mut TestAppContext) -> WindowHandle<Shell> {
    let window = app.update(|cx| cx.open_window(WindowOptions::default(), new_shell).unwrap());
    app.simulate_window_resize(window.into(), size(px(800.), px(600.)));
    AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    window
}

fn activate(window: WindowHandle<Shell>, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
}

fn request_close(window: WindowHandle<Shell>, app: &TestAppContext) {
    let mut visual = VisualTestContext::from_window(window.into(), app);
    assert!(!visual.simulate_close());
    visual.run_until_parked();
}

fn dispatch_quit(app: &TestAppContext) {
    app.update(|cx| cx.dispatch_action(&Quit));
    app.run_until_parked();
}

fn is_close_request_span(label: &str, window_id: u64, intent_substring: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    label.starts_with("window-close-request")
        && label.contains(&format!("window={window_id}"))
        && lower.contains("intent=")
        && lower.contains(intent_substring)
}

fn is_edited_span(label: &str, window_id: u64, extension: &str, edited: bool) -> bool {
    label.starts_with("window-edited")
        && label.contains(&format!("window={window_id}"))
        && label.contains(&format!("extension={extension}"))
        && label.contains(&format!("edited={edited}"))
}

#[derive(Clone, Copy)]
enum BetaMode {
    Cancel,
    PromptThenWait,
    PromptSecondCancelFirst,
    Allow,
}

// Alone in this binary: verifies every logging group, field, and level in
// `docs/src/development/close-guard.md`.
#[gpui::test]
fn close_guard_logging_covers_the_contract_rows(cx: &mut TestAppContext) {
    // Keep tracing's `log` feature from forwarding tracing lines into the
    // diagnostic logger before our scoped subscriber is active.
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let trace = Trace::default();
    let run_trace = trace.clone();
    let check_trace = trace.clone();
    tracing::subscriber::with_default(run_trace, move || {
        let mut app = cx.new_app();
        app.update(|cx| {
            yard_ui::init(cx);
            bind_default_keys(cx);
            handle_quit(cx);
        });

        let (alpha, beta) = app.update(|cx| {
            (
                Extension::new(cx, "alpha").unwrap(),
                Extension::new(cx, "beta").unwrap(),
            )
        });

        // 1. `close-guard-register{extension}`: `DEBUG` registered; `ERROR`
        // for a second guard of the same extension.
        let event_mark = trace.event_mark();
        let stack_mark = trace.stack_mark();
        let alpha_reg = alpha.clone();
        app.update(move |cx| {
            alpha_reg.on_close_request(cx, |_, _, _| CloseDecision::Allow);
        });
        let stacks = trace.stacks_since(stack_mark);
        let events = trace.events_since(event_mark);
        assert!(
            stacks.iter().any(|stack| {
                stack.last().is_some_and(|span| {
                    span.starts_with("close-guard-register") && span.contains("extension=alpha")
                })
            }),
            "missing `close-guard-register{{extension=alpha}}`: {stacks:#?}"
        );
        assert!(
            events.iter().any(|event| {
                event.group().starts_with("close-guard-register")
                    && event.group().contains("extension=alpha")
                    && event.level == tracing::Level::DEBUG
                    && event.fields.contains("registered")
            }),
            "missing `DEBUG registered` in `close-guard-register`: {events:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());

        // Second guard for `alpha` logs exactly one `ERROR` (through `log::error!`
        // and the log bridge) inside `close-guard-register`.
        let event_mark = trace.event_mark();
        let alpha_dup = alpha.clone();
        app.update(move |cx| {
            alpha_dup.on_close_request(cx, |_, _, _| CloseDecision::Cancel);
        });
        let events = trace.events_since(event_mark);
        assert_eq!(errors().len(), 1, "{:?}", errors());
        let duplicate_errors: Vec<_> = events
            .iter()
            .filter(|event| event.level == tracing::Level::ERROR)
            .collect();
        assert_eq!(
            duplicate_errors.len(),
            1,
            "second guard registration must emit exactly one `ERROR` in the trace: {events:#?}"
        );
        assert!(
            duplicate_errors[0]
                .group()
                .starts_with("close-guard-register")
                && duplicate_errors[0].group().contains("extension=alpha"),
            "duplicate-guard `ERROR` must be inside `close-guard-register{{extension=alpha}}`: {events:#?}"
        );

        let first = open_window(&mut app);
        let second = open_window(&mut app);
        let first_id = AnyWindowHandle::from(first).window_id().as_u64();
        let second_id = AnyWindowHandle::from(second).window_id().as_u64();
        let first_handle = AnyWindowHandle::from(first);
        let second_handle = AnyWindowHandle::from(second);

        // Register `beta`'s guard (behaviour switched by `mode`).
        let mode = Rc::new(RefCell::new(BetaMode::Cancel));
        let wait_tx: Rc<RefCell<Option<oneshot::Sender<bool>>>> = Rc::new(RefCell::new(None));
        let mode_in_guard = mode.clone();
        let wait_in_guard = wait_tx.clone();
        let beta_reg = beta.clone();
        app.update(move |cx| {
            beta_reg.on_close_request(cx, move |_, window, _| match *mode_in_guard.borrow() {
                BetaMode::Cancel => CloseDecision::Cancel,
                BetaMode::PromptThenWait => {
                    let wait = wait_in_guard.clone();
                    CloseDecision::prompt(
                        Prompt::new(PromptLevel::Warning, "Save changes?", &["Save", "Cancel"]),
                        move |answer, _, cx| match answer {
                            Some(0) => {
                                let (tx, rx) = oneshot::channel::<bool>();
                                *wait.borrow_mut() = Some(tx);
                                CloseOutcome::Wait(
                                    cx.spawn(async move |_| rx.await.unwrap_or(false)),
                                )
                            }
                            _ => CloseOutcome::Cancel,
                        },
                    )
                }
                BetaMode::PromptSecondCancelFirst => {
                    if window.window_handle() == second_handle {
                        CloseDecision::prompt(
                            Prompt::new(PromptLevel::Warning, "Quit?", &["Allow", "Cancel"]),
                            |answer, _, _| match answer {
                                Some(0) => CloseOutcome::Allow,
                                _ => CloseOutcome::Cancel,
                            },
                        )
                    } else if window.window_handle() == first_handle {
                        CloseDecision::Cancel
                    } else {
                        CloseDecision::Allow
                    }
                }
                BetaMode::Allow => CloseDecision::Allow,
            });
        });
        assert_eq!(errors().len(), 1, "{:?}", errors());

        // 2. `window-edited{window, extension, edited}`: `DEBUG` when the
        // window's combined state changes, `TRACE` otherwise.
        let set_edited = move |ext: &Extension, edited: bool, app: &mut TestAppContext| {
            let ext = ext.clone();
            AnyWindowHandle::from(first)
                .update(app, move |_, window, cx| {
                    ext.set_window_edited(window, edited, cx);
                })
                .unwrap();
        };

        // Combined state changes: false -> true (`DEBUG`).
        let event_mark = trace.event_mark();
        set_edited(&alpha, true, &mut app);
        let events = trace.events_since(event_mark);
        assert!(
            events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "alpha", true)
                    && event.level == tracing::Level::DEBUG
            }),
            "expected `DEBUG` in `window-edited` when combined state changes to true: {events:#?}"
        );

        // Combined state unchanged: true -> true (`TRACE`, no `DEBUG`).
        let event_mark = trace.event_mark();
        set_edited(&beta, true, &mut app);
        let events = trace.events_since(event_mark);
        assert!(
            events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "beta", true)
                    && event.level == tracing::Level::TRACE
            }),
            "expected `TRACE` in `window-edited` when combined state stays true: {events:#?}"
        );
        assert!(
            !events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "beta", true)
                    && event.level == tracing::Level::DEBUG
            }),
            "unchanged combined state must not log `DEBUG`: {events:#?}"
        );

        // Combined state unchanged: true -> true when alpha clears (`TRACE`, no `DEBUG`).
        let event_mark = trace.event_mark();
        set_edited(&alpha, false, &mut app);
        let events = trace.events_since(event_mark);
        assert!(
            events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "alpha", false)
                    && event.level == tracing::Level::TRACE
            }),
            "expected `TRACE` in `window-edited` when alpha clears while beta keeps edited: {events:#?}"
        );
        assert!(
            !events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "alpha", false)
                    && event.level == tracing::Level::DEBUG
            }),
            "unchanged combined state must not log `DEBUG`: {events:#?}"
        );

        // Combined state changes: true -> false when beta clears (`DEBUG`).
        let event_mark = trace.event_mark();
        set_edited(&beta, false, &mut app);
        let events = trace.events_since(event_mark);
        assert!(
            events.iter().any(move |event| {
                is_edited_span(event.group(), first_id, "beta", false)
                    && event.level == tracing::Level::DEBUG
            }),
            "expected `DEBUG` in `window-edited` when combined state changes to false: {events:#?}"
        );

        // 3a. `window-close-request{window, intent}` with `DEBUG` per guard
        // (`allowed`, `cancelled`) and `INFO` `cancelled` naming the extension.
        *mode.borrow_mut() = BetaMode::Cancel;
        let event_mark = trace.event_mark();
        let stack_mark = trace.stack_mark();
        request_close(first, &app);
        let stacks = trace.stacks_since(stack_mark);
        let events = trace.events_since(event_mark);
        assert!(
            stacks.iter().any(move |stack| stack
                .last()
                .is_some_and(move |span| is_close_request_span(span, first_id, "close"))),
            "missing `window-close-request{{window={first_id}, intent=...}}`: {stacks:#?}"
        );
        assert!(
            events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::DEBUG
                    && event.fields.contains("allowed")
            }),
            "missing `DEBUG allowed` in `window-close-request`: {events:#?}"
        );
        assert!(
            events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::DEBUG
                    && event.fields.contains("cancelled")
            }),
            "missing `DEBUG cancelled` in `window-close-request`: {events:#?}"
        );
        assert!(
            events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::INFO
                    && event.fields.contains("cancelled")
                    && event.fields.contains("beta")
            }),
            "missing `INFO cancelled` naming `beta` in `window-close-request`: {events:#?}"
        );

        // 3b. `DEBUG prompted`, `INFO already in progress`, and `DEBUG waiting`.
        *mode.borrow_mut() = BetaMode::PromptThenWait;
        let event_mark = trace.event_mark();
        request_close(first, &app);
        assert!(app.has_pending_prompt());
        let events = trace.events_since(event_mark);
        assert!(
            events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::DEBUG
                    && event.fields.contains("prompted")
            }),
            "missing `DEBUG prompted` in `window-close-request`: {events:#?}"
        );

        let repeat_mark = trace.event_mark();
        request_close(first, &app);
        let repeat_events = trace.events_since(repeat_mark);
        assert!(
            repeat_events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::INFO
                    && event.fields.contains("already in progress")
            }),
            "missing `INFO already in progress` for repeated window close: {repeat_events:#?}"
        );

        let quit_during_close_mark = trace.event_mark();
        dispatch_quit(&app);
        let quit_during_close_events = trace.events_since(quit_during_close_mark);
        assert!(
            quit_during_close_events.iter().any(|event| {
                event.group().starts_with("app-quit")
                    && event.level == tracing::Level::INFO
                    && event.fields.contains("a close is in progress")
            }),
            "missing `INFO a close is in progress` for Quit during close: {quit_during_close_events:#?}"
        );

        let wait_mark = trace.event_mark();
        app.simulate_prompt_answer("Save");
        app.run_until_parked();
        let wait_events = trace.events_since(wait_mark);
        assert!(
            wait_events.iter().any(move |event| {
                is_close_request_span(event.group(), first_id, "close")
                    && event.level == tracing::Level::DEBUG
                    && event.fields.contains("waiting")
            }),
            "missing `DEBUG waiting` in `window-close-request`: {wait_events:#?}"
        );
        // Resolve the wait as `false` so `first` stays open for the quit checks.
        wait_tx.borrow_mut().take().unwrap().send(false).unwrap();
        app.run_until_parked();
        assert_eq!(app.read(|cx| cx.windows().len()), 2);

        // 4a. `app-quit` surrounds the guard pass over both windows with
        // `windows=2`, logs `INFO already in progress` for a repeated quit,
        // and ends `cancelled` when a window cancels.
        activate(second, &mut app);
        *mode.borrow_mut() = BetaMode::PromptSecondCancelFirst;
        let event_mark = trace.event_mark();
        let stack_mark = trace.stack_mark();
        dispatch_quit(&app);
        assert!(app.has_pending_prompt());

        let repeat_quit_mark = trace.event_mark();
        dispatch_quit(&app);
        let repeat_quit_events = trace.events_since(repeat_quit_mark);
        assert!(
            repeat_quit_events.iter().any(|event| {
                event.group().starts_with("app-quit")
                    && event.level == tracing::Level::INFO
                    && event.fields.contains("already in progress")
            }),
            "missing `INFO already in progress` for repeated Quit: {repeat_quit_events:#?}"
        );

        app.simulate_prompt_answer("Allow");
        app.run_until_parked();
        let stacks = trace.stacks_since(stack_mark);
        let events = trace.events_since(event_mark);

        let has_windows_two = stacks.iter().any(|stack| {
            stack
                .iter()
                .any(|span| span.starts_with("app-quit") && span.contains("windows=2"))
        }) || events.iter().any(|event| {
            event.group().starts_with("app-quit") && event.fields.contains("windows=2")
        });
        assert!(
            has_windows_two,
            "`app-quit` must record `windows=2`: spans={stacks:#?}, events={events:#?}"
        );

        for window_id in [second_id, first_id] {
            assert!(
                stacks.iter().any(move |stack| {
                    stack.iter().any(|span| span.starts_with("app-quit"))
                        && stack
                            .last()
                            .is_some_and(move |span| is_close_request_span(span, window_id, "quit"))
                }),
                "`app-quit` must surround `window-close-request{{window={window_id}, intent=quit}}`: {stacks:#?}"
            );
        }
        assert!(
            events.iter().any(|event| {
                event.group().starts_with("app-quit")
                    && matches!(event.level, tracing::Level::INFO | tracing::Level::DEBUG)
                    && event.fields.contains("cancelled")
            }),
            "cancelled `app-quit` must log `cancelled`: {events:#?}"
        );
        assert!(
            !events.iter().any(|event| {
                event.group().starts_with("app-quit") && event.fields.contains("quitting")
            }),
            "cancelled `app-quit` must not log `quitting`: {events:#?}"
        );

        // 4b. `app-quit` ends `quitting` when every window allows.
        *mode.borrow_mut() = BetaMode::Allow;
        let event_mark = trace.event_mark();
        let stack_mark = trace.stack_mark();
        dispatch_quit(&app);
        let stacks = trace.stacks_since(stack_mark);
        let events = trace.events_since(event_mark);
        let has_windows_two = stacks.iter().any(|stack| {
            stack
                .iter()
                .any(|span| span.starts_with("app-quit") && span.contains("windows=2"))
        }) || events.iter().any(|event| {
            event.group().starts_with("app-quit") && event.fields.contains("windows=2")
        });
        assert!(
            has_windows_two,
            "`app-quit` must record `windows=2`: spans={stacks:#?}, events={events:#?}"
        );
        assert!(
            events.iter().any(|event| {
                event.group().starts_with("app-quit")
                    && matches!(event.level, tracing::Level::INFO | tracing::Level::DEBUG)
                    && event.fields.contains("quitting")
            }),
            "allowed `app-quit` must log `quitting`: {events:#?}"
        );
        assert!(
            !events.iter().any(|event| {
                event.group().starts_with("app-quit") && event.fields.contains("cancelled")
            }),
            "allowed `app-quit` must not log `cancelled`: {events:#?}"
        );

        // Only the intentional duplicate-registration error occurred.
        assert_eq!(errors().len(), 1, "{:?}", errors());
    });

    assert!(
        check_trace
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .is_empty()
    );
}
