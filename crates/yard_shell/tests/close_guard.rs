#![cfg(feature = "dev")]

use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, App, PromptLevel, TestAppContext, VisualTestContext, Window, WindowHandle,
    WindowOptions, px, size,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, Once},
    time::Duration,
};
use yard_shell::{
    CloseDecision, CloseIntent, CloseOutcome, Extension, Prompt, Quit, Shell, bind_default_keys,
    handle_quit, is_window_edited, new_shell,
};

// Every test in this file shares one process-wide diagnostic logger and runs
// under one lock with its own lifecycle subscriber, so no test sees another
// test's records and no thread reaches the shell's log lines without a
// subscriber.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        // Keep tracing's `log` bridge out of the diagnostic logger before any
        // scoped subscriber is installed.
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    guard
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
    lines: Vec<String>,
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
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
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
        if !event.metadata().target().starts_with("yard_shell")
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let group = captured
            .entered
            .last()
            .and_then(|id| captured.spans.get(id))
            .map(String::as_str)
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
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
    fn mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .len()
    }

    fn since(&self, mark: usize) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines[mark..]
            .iter()
            .map(|line| line.replace('"', ""))
            .collect()
    }

    fn span_mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .len()
    }

    fn spans_since(&self, mark: usize) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .values()
            .skip(mark)
            .map(|span| span.replace('"', ""))
            .collect()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = lock();
    let trace = Trace::default();
    let run = trace.clone();
    let check = trace.clone();
    tracing::subscriber::with_default(run, move || test(cx, &trace));
    assert!(
        check
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .is_empty()
    );
}

fn setup_shell(app: &mut TestAppContext) {
    app.update(|cx| {
        yard_ui::init(cx);
        bind_default_keys(cx);
        handle_quit(cx);
    });
}

fn draw(window: WindowHandle<Shell>, app: &mut TestAppContext) {
    AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_window(app: &mut TestAppContext) -> WindowHandle<Shell> {
    let window = app.update(|cx| cx.open_window(WindowOptions::default(), new_shell).unwrap());
    app.simulate_window_resize(window.into(), size(px(800.), px(600.)));
    draw(window, app);
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
    assert!(
        !visual.simulate_close(),
        "the shell handles closing itself rather than letting the platform close directly"
    );
    visual.run_until_parked();
}

fn dispatch_quit(app: &TestAppContext) {
    app.update(|cx| cx.dispatch_action(&Quit));
    app.run_until_parked();
}

fn save_prompt() -> Prompt {
    Prompt::new(
        PromptLevel::Warning,
        "Save changes?",
        &["Save", "Don't Save", "Cancel"],
    )
}

fn app_quit_logged(lines: &[String], outcome: &str) -> bool {
    lines.iter().any(move |line| {
        let Some((group, rest)) = line.split_once(": ") else {
            return false;
        };
        group.starts_with("app-quit")
            && (rest.starts_with("DEBUG ") || rest.starts_with("INFO "))
            && rest.contains(outcome)
    })
}

fn app_quit_logged_info(lines: &[String], message: &str) -> bool {
    lines.iter().any(move |line| {
        let Some((group, rest)) = line.split_once(": ") else {
            return false;
        };
        group.starts_with("app-quit") && rest.starts_with("INFO ") && rest.contains(message)
    })
}

fn window_close_logged(lines: &[String], message: &str) -> bool {
    lines.iter().any(move |line| {
        let Some((group, rest)) = line.split_once(": ") else {
            return false;
        };
        group.starts_with("window-close-request")
            && (rest.starts_with("DEBUG ") || rest.starts_with("INFO "))
            && rest.contains(message)
    })
}

// 1. No guard registered: closing a window and quitting behave as today.
#[gpui::test]
fn no_guard_closes_window_and_quits_as_today(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);

        let quits = Rc::new(Cell::new(0usize));
        let quits_in_cb = quits.clone();
        app.update(move |cx| {
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    quits_in_cb.set(quits_in_cb.get() + 1);
                    cx.quit();
                }
            })
            .detach();
        });

        let first = open_window(&mut app);
        let second = open_window(&mut app);
        assert_eq!(app.read(|cx| cx.windows().len()), 2);

        request_close(first, &app);
        assert!(!app.has_pending_prompt());
        assert_eq!(
            app.read(|cx| cx.windows()),
            vec![AnyWindowHandle::from(second)],
            "closing the first window removes it without a guard"
        );
        assert_eq!(quits.get(), 0);

        // `Quit` runs directly into `app-quit` and quits without a prompt.
        let line_mark = trace.mark();
        let span_mark = trace.span_mark();
        dispatch_quit(&app);
        assert!(!app.has_pending_prompt());
        let spans = trace.spans_since(span_mark);
        let lines = trace.since(line_mark);
        assert!(
            spans.iter().any(|span| span.starts_with("app-quit")),
            "Quit must open `app-quit`: {spans:#?}"
        );
        assert!(
            app_quit_logged(&lines, "quitting"),
            "Quit with no guard must log `quitting`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "Quit with no guard must not be cancelled: {lines:#?}"
        );

        // Closing the remaining window empties the window list and triggers
        // the last-window quit once.
        request_close(second, &app);
        assert!(app.read(|cx| cx.windows().is_empty()));
        assert_eq!(quits.get(), 1);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2a. One guard returning `Allow` closes the window.
#[gpui::test]
fn guard_allow_closes_the_window(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        app.update(|cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, |_, _, _| CloseDecision::Allow);
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(!app.has_pending_prompt());
        assert!(
            app.read(|cx| cx.windows().is_empty()),
            "an Allow guard must close the window"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2b. One guard returning `Cancel` keeps the window open.
#[gpui::test]
fn guard_cancel_keeps_the_window_open(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        app.update(|cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, |_, _, _| CloseDecision::Cancel);
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(!app.has_pending_prompt());
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "a Cancel guard must keep the window open"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2c. One guard returning `Prompt`, answered "Save", whose `on_answer` returns
// `Wait` of a task resolving `true`: the window closes only after the task.
#[gpui::test]
fn guard_prompt_save_waits_for_task_then_closes(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let pending: Rc<RefCell<Option<oneshot::Sender<bool>>>> = Rc::new(RefCell::new(None));
        let pending_in_guard = pending.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, _| {
                let pending = pending_in_guard.clone();
                CloseDecision::prompt(save_prompt(), move |answer, _, cx| match answer {
                    Some(0) => {
                        let (tx, rx) = oneshot::channel::<bool>();
                        *pending.borrow_mut() = Some(tx);
                        CloseOutcome::Wait(cx.spawn(async move |_| rx.await.unwrap_or(false)))
                    }
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(app.has_pending_prompt(), "Prompt guard must show a prompt");
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        app.simulate_prompt_answer("Save");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert!(
            pending.borrow().is_some(),
            "answering Save must start the Wait task"
        );
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "the window must stay open while the Wait task is pending"
        );

        pending.borrow_mut().take().unwrap().send(true).unwrap();
        app.run_until_parked();
        assert!(
            app.read(|cx| cx.windows().is_empty()),
            "the window must close once the Wait task resolves true"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2d. One guard returning `Prompt`, answered "Don't Save" (`Allow`): closes.
#[gpui::test]
fn guard_prompt_dont_save_allows_and_closes(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        app.update(|cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, |_, _, _| {
                CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(app.has_pending_prompt());
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        app.simulate_prompt_answer("Don't Save");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert!(
            app.read(|cx| cx.windows().is_empty()),
            "answering Don't Save (Allow) must close the window"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2e. One guard returning `Prompt`, answered "Cancel": stays open.
#[gpui::test]
fn guard_prompt_cancel_keeps_the_window_open(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        app.update(|cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, |_, _, _| {
                CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(app.has_pending_prompt());

        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "answering Cancel must keep the window open"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2f. One guard returning `Wait(false)`: stays open, and a later close runs.
#[gpui::test]
fn guard_wait_false_keeps_the_window_open(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let pending: Rc<RefCell<Option<oneshot::Sender<bool>>>> = Rc::new(RefCell::new(None));
        let allow_next = Rc::new(Cell::new(false));
        let pending_in_guard = pending.clone();
        let allow_in_guard = allow_next.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, cx| {
                if allow_in_guard.get() {
                    return CloseDecision::Allow;
                }
                let (tx, rx) = oneshot::channel::<bool>();
                *pending_in_guard.borrow_mut() = Some(tx);
                CloseDecision::Wait(cx.spawn(async move |_| rx.await.unwrap_or(false)))
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(pending.borrow().is_some());
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        pending.borrow_mut().take().unwrap().send(false).unwrap();
        app.run_until_parked();
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "Wait resolving false must keep the window open"
        );

        // The in-progress state was cleared, so a later close can proceed.
        allow_next.set(true);
        request_close(window, &app);
        assert!(app.read(|cx| cx.windows().is_empty()));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 2g. A prompt closed without an answer (`None`) counts as `Cancel` and keeps
// the window open.
#[gpui::test]
fn guard_prompt_closed_without_answer_keeps_the_window_open(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let allow_next = Rc::new(Cell::new(false));
        let allow_in_guard = allow_next.clone();
        app.update(move |cx| {
            // Use GPUI's in-window fallback prompt so the test can dismiss the
            // prompt without selecting a button (dropping its sender -> `None`).
            cx.set_prompt_builder(gpui::fallback_prompt_renderer);
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, _| {
                if allow_in_guard.get() {
                    return CloseDecision::Allow;
                }
                CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(
            window
                .update(&mut app, |_, window, _| window.has_active_prompt())
                .unwrap(),
            "the custom prompt must be active"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        // Overwriting the window's active prompt drops the first prompt's
        // view and its oneshot sender, resolving the receiver with `None`.
        window
            .update(&mut app, |_, window, cx| {
                drop(window.prompt(PromptLevel::Info, "dismiss", None, &["OK"], cx));
            })
            .unwrap();
        app.run_until_parked();
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "a prompt closed without an answer must keep the window open"
        );

        // Once cancelled, the in-progress state is cleared and a later close works.
        allow_next.set(true);
        app.update(|cx| cx.reset_prompt_builder());
        request_close(window, &app);
        assert!(app.read(|cx| cx.windows().is_empty()));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 3. The intent: `CloseWindow` for a window close, `Quit` for the `Quit` action.
#[gpui::test]
fn guard_receives_close_window_for_close_and_quit_for_quit_action(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let intents: Rc<RefCell<Vec<CloseIntent>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = intents.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, _, _| {
                recorded.borrow_mut().push(intent);
                CloseDecision::Cancel
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        dispatch_quit(&app);

        assert_eq!(
            *intents.borrow(),
            vec![CloseIntent::CloseWindow, CloseIntent::Quit],
            "window close must pass CloseWindow and Quit action must pass Quit"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 4. Two extensions' guards run in the order the extensions were listed.
// When the first cancels, the second is not called.
#[gpui::test]
fn two_extensions_guards_run_in_order_and_stop_when_first_cancels(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let calls: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let first_cancels = Rc::new(Cell::new(true));

        let calls_alpha = calls.clone();
        let cancel_alpha = first_cancels.clone();
        let calls_beta = calls.clone();
        app.update(move |cx| {
            let alpha = Extension::new(cx, "alpha").unwrap();
            let beta = Extension::new(cx, "beta").unwrap();
            alpha.on_close_request(cx, move |_, _, _| {
                calls_alpha.borrow_mut().push("alpha");
                if cancel_alpha.get() {
                    CloseDecision::Cancel
                } else {
                    CloseDecision::Allow
                }
            });
            beta.on_close_request(cx, move |_, _, _| {
                calls_beta.borrow_mut().push("beta");
                CloseDecision::Allow
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert_eq!(
            *calls.borrow(),
            vec!["alpha"],
            "when the first guard cancels, the second must not be called"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        calls.borrow_mut().clear();
        first_cancels.set(false);
        request_close(window, &app);
        assert_eq!(
            *calls.borrow(),
            vec!["alpha", "beta"],
            "guards must run in extension listing order"
        );
        assert!(app.read(|cx| cx.windows().is_empty()));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 5. Quit with two windows: the active window's guard runs first. When the
// second window cancels, both windows stay open and the app does not quit.
// When both allow, the app quits.
#[gpui::test]
fn quit_with_two_windows_runs_active_first_cancels_all_or_quits_when_both_allow(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let second = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);
        let second_handle = AnyWindowHandle::from(second);

        // Make the second-opened window active so "active first" differs from
        // opening order.
        activate(second, &mut app);
        assert_eq!(app.read(|cx| cx.active_window()), Some(second_handle));

        let calls: Rc<RefCell<Vec<(AnyWindowHandle, CloseIntent)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let cancel_first = Rc::new(Cell::new(true));
        let calls_in_guard = calls.clone();
        let cancel_in_guard = cancel_first.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, window, _| {
                let handle = window.window_handle();
                calls_in_guard.borrow_mut().push((handle, intent));
                if handle == first_handle && cancel_in_guard.get() {
                    CloseDecision::Cancel
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        let lines = trace.since(mark);
        assert_eq!(
            *calls.borrow(),
            vec![
                (second_handle, CloseIntent::Quit),
                (first_handle, CloseIntent::Quit),
            ],
            "the active window's guard must run first, then the other window"
        );
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            2,
            "both windows must stay open when the second window cancels"
        );
        assert!(
            app_quit_logged(&lines, "cancelled"),
            "cancelled Quit must log `cancelled` in `app-quit`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "quitting"),
            "cancelled Quit must not quit: {lines:#?}"
        );

        // Now allow on both windows: the application quits.
        calls.borrow_mut().clear();
        cancel_first.set(false);
        let mark = trace.mark();
        dispatch_quit(&app);
        let lines = trace.since(mark);
        assert_eq!(
            *calls.borrow(),
            vec![
                (second_handle, CloseIntent::Quit),
                (first_handle, CloseIntent::Quit),
            ]
        );
        assert!(
            app_quit_logged(&lines, "quitting"),
            "Quit with both windows allowing must log `quitting`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "allowed Quit must not log `cancelled`: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 6. The last window: when its guards allow, the application quits once, and
// the guards ran once, with `CloseWindow`.
#[gpui::test]
fn closing_the_last_window_quits_once_and_runs_guards_once_with_close_window(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let quits = Rc::new(Cell::new(0usize));
        let intents: Rc<RefCell<Vec<CloseIntent>>> = Rc::new(RefCell::new(Vec::new()));

        let quits_in_cb = quits.clone();
        let intents_in_guard = intents.clone();
        app.update(move |cx| {
            // Matches `yard_app/src/startup.rs`: closing the last window quits.
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    quits_in_cb.set(quits_in_cb.get() + 1);
                    cx.quit();
                }
            })
            .detach();
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, _, _| {
                intents_in_guard.borrow_mut().push(intent);
                CloseDecision::Allow
            });
        });

        let window = open_window(&mut app);
        let span_mark = trace.span_mark();
        request_close(window, &app);

        assert!(app.read(|cx| cx.windows().is_empty()));
        assert_eq!(
            quits.get(),
            1,
            "closing the last window must quit the application once"
        );
        assert_eq!(
            *intents.borrow(),
            vec![CloseIntent::CloseWindow],
            "guards must run once with CloseWindow and not run again for Quit"
        );
        let spans = trace.spans_since(span_mark);
        assert!(
            !spans.iter().any(|span| span.starts_with("app-quit")),
            "closing the last window must not run a second app-quit pass: {spans:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 7. No stacking: a second close request for the same window while its prompt
// is open shows no second prompt; likewise for a second `Quit`.
#[gpui::test]
fn repeated_close_or_quit_while_prompt_is_open_does_not_stack(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let calls = Rc::new(Cell::new(0usize));
        let calls_in_guard = calls.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, _| {
                calls_in_guard.set(calls_in_guard.get() + 1);
                CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        // Two close requests while the first prompt is still open.
        request_close(window, &app);
        assert!(app.has_pending_prompt());
        assert_eq!(calls.get(), 1);

        request_close(window, &app);
        assert_eq!(
            calls.get(),
            1,
            "a second close request while a prompt is open must be ignored"
        );

        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(
            !app.has_pending_prompt(),
            "no second prompt may be stacked after answering the first"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);

        // Two `Quit` actions while the first prompt is still open.
        calls.set(0);
        dispatch_quit(&app);
        assert!(app.has_pending_prompt());
        assert_eq!(calls.get(), 1);

        dispatch_quit(&app);
        assert_eq!(
            calls.get(),
            1,
            "a second Quit while a quit prompt is open must be ignored"
        );

        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(
            !app.has_pending_prompt(),
            "no second prompt may be stacked for Quit"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 8. `yard::CloseWindow` dispatched to a window runs the same guards as the
// system's close.
#[gpui::test]
fn close_window_action_runs_the_same_guards_as_system_close(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let intents: Rc<RefCell<Vec<CloseIntent>>> = Rc::new(RefCell::new(Vec::new()));
        let cancel = Rc::new(Cell::new(true));
        let intents_in_guard = intents.clone();
        let cancel_in_guard = cancel.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, _, _| {
                intents_in_guard.borrow_mut().push(intent);
                if cancel_in_guard.get() {
                    CloseDecision::Cancel
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let window = open_window(&mut app);
        activate(window, &mut app);

        window
            .update(&mut app, |_, window, cx| {
                window.dispatch_action(Box::new(yard_ui::CloseWindow), cx);
            })
            .unwrap();
        app.run_until_parked();
        assert_eq!(*intents.borrow(), vec![CloseIntent::CloseWindow]);
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            1,
            "yard::CloseWindow must respect a cancelling guard"
        );

        cancel.set(false);
        window
            .update(&mut app, |_, window, cx| {
                window.dispatch_action(Box::new(yard_ui::CloseWindow), cx);
            })
            .unwrap();
        app.run_until_parked();
        assert_eq!(
            *intents.borrow(),
            vec![CloseIntent::CloseWindow, CloseIntent::CloseWindow]
        );
        assert!(
            app.read(|cx| cx.windows().is_empty()),
            "yard::CloseWindow must close the window once guards allow"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 9. The edited state combines extensions per window without overwriting each
// other, and leaves other windows unaffected.
#[gpui::test]
fn window_edited_combines_extensions_per_window(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let (alpha, beta) = app.update(|cx| {
            (
                Extension::new(cx, "alpha").unwrap(),
                Extension::new(cx, "beta").unwrap(),
            )
        });
        let window_a = open_window(&mut app);
        let window_b = open_window(&mut app);

        let edited = |window: WindowHandle<Shell>, app: &mut TestAppContext| -> bool {
            AnyWindowHandle::from(window)
                .update(app, |_, window: &mut Window, cx: &mut App| {
                    is_window_edited(window, cx)
                })
                .unwrap()
        };
        let set_edited = |ext: &Extension,
                          window: WindowHandle<Shell>,
                          value: bool,
                          app: &mut TestAppContext| {
            let ext = ext.clone();
            AnyWindowHandle::from(window)
                .update(app, move |_, window: &mut Window, cx: &mut App| {
                    ext.set_window_edited(window, value, cx);
                })
                .unwrap();
        };

        assert!(!edited(window_a, &mut app));
        assert!(!edited(window_b, &mut app));

        set_edited(&alpha, window_a, true, &mut app);
        assert!(edited(window_a, &mut app));
        assert!(
            !edited(window_b, &mut app),
            "edited state belongs to its window"
        );

        set_edited(&beta, window_a, true, &mut app);
        assert!(edited(window_a, &mut app));
        assert!(!edited(window_b, &mut app));

        set_edited(&alpha, window_a, false, &mut app);
        assert!(
            edited(window_a, &mut app),
            "window stays edited while beta still marks it edited"
        );
        assert!(!edited(window_b, &mut app));

        set_edited(&beta, window_a, false, &mut app);
        assert!(
            !edited(window_a, &mut app),
            "window clears edited once every extension clears it"
        );
        assert!(!edited(window_b, &mut app));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 10. A second `on_close_request` from the same extension is refused with an
// `ERROR`, and the first guard still runs.
#[gpui::test]
fn second_on_close_request_from_same_extension_is_refused_with_error(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first_calls = Rc::new(Cell::new(0usize));
        let second_calls = Rc::new(Cell::new(0usize));
        let first_in_guard = first_calls.clone();
        let second_in_guard = second_calls.clone();

        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, _| {
                first_in_guard.set(first_in_guard.get() + 1);
                CloseDecision::Cancel
            });
            assert!(errors().is_empty(), "{:?}", errors());
            ext.on_close_request(cx, move |_, _, _| {
                second_in_guard.set(second_in_guard.get() + 1);
                CloseDecision::Allow
            });
        });

        assert_eq!(
            errors().len(),
            1,
            "the second registration must be refused with an ERROR: {:?}",
            errors()
        );

        let window = open_window(&mut app);
        request_close(window, &app);
        assert_eq!(
            first_calls.get(),
            1,
            "the first guard must still run after a refused second registration"
        );
        assert_eq!(
            second_calls.get(),
            0,
            "the refused second guard must not run"
        );
        assert_eq!(app.read(|cx| cx.windows().len()), 1);
        assert_eq!(errors().len(), 1, "{:?}", errors());
    });
}

// 11. Window closed during the quit pass is skipped, not counted as a cancel.
#[gpui::test]
fn quit_skips_window_closed_during_the_pass(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let second = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);

        activate(first, &mut app);
        assert_eq!(app.read(|cx| cx.active_window()), Some(first_handle));

        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, window, _| {
                if window.window_handle() == first_handle {
                    CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                        Some(1) => CloseOutcome::Allow,
                        _ => CloseOutcome::Cancel,
                    })
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        assert!(app.has_pending_prompt());

        second
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        assert_eq!(app.read(|cx| cx.windows()), vec![first_handle]);
        assert!(app.has_pending_prompt());

        app.simulate_prompt_answer("Don't Save");
        app.run_until_parked();
        let lines = trace.since(mark);
        assert!(!app.has_pending_prompt());
        assert!(
            app_quit_logged(&lines, "quitting"),
            "removing another window during the quit pass must not cancel quit: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "removing another window during the quit pass must not log `cancelled`: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 12. Window opened during the quit pass gets its guards too.
#[gpui::test]
fn quit_checks_window_opened_during_the_pass(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);

        let calls: Rc<RefCell<Vec<(AnyWindowHandle, CloseIntent)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let calls_in_guard = calls.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, window, _| {
                let handle = window.window_handle();
                calls_in_guard.borrow_mut().push((handle, intent));
                if handle == first_handle {
                    CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                        Some(1) => CloseOutcome::Allow,
                        _ => CloseOutcome::Cancel,
                    })
                } else {
                    CloseDecision::Cancel
                }
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        assert!(app.has_pending_prompt());

        let opened = open_window(&mut app);
        let opened_handle = AnyWindowHandle::from(opened);

        app.simulate_prompt_answer("Don't Save");
        app.run_until_parked();
        let lines = trace.since(mark);
        assert!(!app.has_pending_prompt());
        assert_eq!(
            *calls.borrow(),
            vec![
                (first_handle, CloseIntent::Quit),
                (opened_handle, CloseIntent::Quit),
            ],
            "a window opened during the quit pass must have its guards run with Quit"
        );
        assert_eq!(
            app.read(|cx| cx.windows().len()),
            2,
            "both windows must stay open when the newly opened window cancels quit"
        );
        assert!(
            app_quit_logged(&lines, "cancelled"),
            "cancelling on a window opened during the quit pass must log `cancelled`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "quitting"),
            "cancelling on a window opened during the quit pass must not quit: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 13. Dispatching `Quit` while a window's close guard is prompting does nothing
// and logs `info` `a close is in progress` in `app-quit`.
#[gpui::test]
fn quit_during_close_prompt_does_nothing_and_logs_close_in_progress(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let calls = Rc::new(Cell::new(0usize));
        let calls_in_guard = calls.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, _| {
                calls_in_guard.set(calls_in_guard.get() + 1);
                CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                    Some(1) => CloseOutcome::Allow,
                    _ => CloseOutcome::Cancel,
                })
            });
        });
        let window = open_window(&mut app);

        request_close(window, &app);
        assert!(app.has_pending_prompt());
        assert_eq!(calls.get(), 1);

        let mark = trace.mark();
        dispatch_quit(&app);
        let lines = trace.since(mark);
        assert_eq!(
            calls.get(),
            1,
            "Quit during a close prompt must not invoke guards"
        );
        assert!(app.has_pending_prompt());
        assert_eq!(app.read(|cx| cx.windows().len()), 1);
        assert!(
            lines.iter().any(|line| {
                let Some((group, rest)) = line.split_once(": ") else {
                    return false;
                };
                group.starts_with("app-quit")
                    && rest.starts_with("INFO ")
                    && rest.contains("a close is in progress")
            }),
            "Quit during a close prompt must log `INFO a close is in progress` in `app-quit`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "quitting"),
            "Quit during a close prompt must not quit: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "Quit during a close prompt must not run a quit pass: {lines:#?}"
        );

        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert_eq!(app.read(|cx| cx.windows().len()), 1);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 14a. A `Wait` task during `Quit` runs to completion even if its window is
// removed while it waits, and the quit pass finishes after it.
#[gpui::test]
fn quit_wait_task_survives_window_removal(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let _second = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);

        activate(first, &mut app);
        assert_eq!(app.read(|cx| cx.active_window()), Some(first_handle));

        let saved = Rc::new(Cell::new(false));
        let saved_in_guard = saved.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, window, cx| {
                if window.window_handle() == first_handle {
                    let saved_in_task = saved_in_guard.clone();
                    CloseDecision::Wait(cx.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        saved_in_task.set(true);
                        true
                    }))
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        assert!(
            !saved.get(),
            "the Wait task must still be pending before the clock advances"
        );

        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        let quit_before_wait_finished = app_quit_logged(&trace.since(mark), "quitting");

        app.executor().advance_clock(Duration::from_secs(2));
        app.run_until_parked();
        let lines = trace.since(mark);

        assert!(
            saved.get(),
            "a Wait task must complete even when its window is removed while waiting"
        );
        assert!(
            !quit_before_wait_finished,
            "Quit must wait for the Wait task to finish before quitting: {lines:#?}"
        );
        assert!(
            app_quit_logged(&lines, "quitting"),
            "Quit must log `quitting` after the removed window's Wait task finishes: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "removing the window while its Wait task runs must not cancel Quit: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 14b. A `Wait` task during a `CloseWindow` request runs to completion even if
// its window is removed while it waits.
#[gpui::test]
fn close_window_wait_task_survives_window_removal(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let window = open_window(&mut app);

        let saved = Rc::new(Cell::new(false));
        let saved_in_guard = saved.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |_, _, cx| {
                let saved_in_task = saved_in_guard.clone();
                CloseDecision::Wait(cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    saved_in_task.set(true);
                    true
                }))
            });
        });

        request_close(window, &app);
        assert!(
            !saved.get(),
            "the Wait task must still be pending before the clock advances"
        );

        window
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        assert!(
            !saved.get(),
            "the Wait task must still be waiting on the clock after the window is removed"
        );

        app.executor().advance_clock(Duration::from_secs(2));
        app.run_until_parked();

        assert!(
            saved.get(),
            "a CloseWindow Wait task must complete even when its window is removed while waiting"
        );
        assert!(app.read(|cx| cx.windows().is_empty()));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 15. The quit re-scan is bounded to at most 8 rounds, logs `info`
// `windows keep opening` and `cancelled`, and clears the in-progress state so
// a later `Quit` can run.
#[gpui::test]
fn quit_rescan_is_bounded_and_allows_later_quit(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        const CAP: usize = 50;

        let mut app = cx.new_app();
        setup_shell(&mut app);
        let _first = open_window(&mut app);

        let calls = Rc::new(Cell::new(0usize));
        let keep_opening = Rc::new(Cell::new(true));
        let calls_in_guard = calls.clone();
        let keep_opening_in_guard = keep_opening.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, _, cx| {
                if intent == CloseIntent::Quit {
                    let count = calls_in_guard.get() + 1;
                    calls_in_guard.set(count);
                    if keep_opening_in_guard.get() && count <= CAP {
                        cx.open_window(WindowOptions::default(), new_shell).unwrap();
                    }
                }
                CloseDecision::Allow
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        let lines = trace.since(mark);

        assert!(
            app_quit_logged_info(&lines, "windows keep opening"),
            "exceeding 8 re-scan rounds must log `INFO windows keep opening` in `app-quit`: {lines:#?}"
        );
        assert!(
            app_quit_logged(&lines, "cancelled"),
            "exceeding 8 re-scan rounds must log `cancelled` in `app-quit`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "quitting"),
            "exceeding 8 re-scan rounds must not log `quitting`: {lines:#?}"
        );
        assert!(
            calls.get() > 0 && calls.get() <= 8,
            "guard must be called no more than the windows checked in 8 rounds, was called {} times",
            calls.get()
        );

        // Once cancelled, the in-progress flag must be cleared so a later `Quit` runs.
        keep_opening.set(false);
        let mark = trace.mark();
        dispatch_quit(&app);
        let lines = trace.since(mark);

        assert!(
            !app_quit_logged(&lines, "already in progress"),
            "a later Quit must not be refused as `already in progress`: {lines:#?}"
        );
        assert!(
            app_quit_logged(&lines, "quitting"),
            "a later Quit after bounded cancel must succeed and log `quitting`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "a later allowed Quit must not log `cancelled`: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 16a. During `Quit`, removing a window while its native/default test prompt is
// open stops waiting for the prompt, skips the gone window, checks the other,
// and finishes quitting.
#[gpui::test]
fn quit_prompt_stops_waiting_when_window_is_removed(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let second = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);
        let second_handle = AnyWindowHandle::from(second);

        activate(first, &mut app);
        assert_eq!(app.read(|cx| cx.active_window()), Some(first_handle));

        let calls: Rc<RefCell<Vec<(AnyWindowHandle, CloseIntent)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let calls_in_guard = calls.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, window, _| {
                let handle = window.window_handle();
                calls_in_guard.borrow_mut().push((handle, intent));
                if handle == first_handle {
                    CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                        Some(1) => CloseOutcome::Allow,
                        _ => CloseOutcome::Cancel,
                    })
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let mark = trace.mark();
        dispatch_quit(&app);
        assert!(app.has_pending_prompt());
        assert_eq!(*calls.borrow(), vec![(first_handle, CloseIntent::Quit)]);

        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();

        let lines = trace.since(mark);
        assert!(
            window_close_logged(&lines, "already closed"),
            "removing the prompting window must skip it with `already closed`: {lines:#?}"
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                (first_handle, CloseIntent::Quit),
                (second_handle, CloseIntent::Quit),
            ],
            "removing the prompting window must skip it and run guards on the remaining window: {lines:#?}"
        );
        assert!(
            app_quit_logged(&lines, "quitting"),
            "removing the prompting window during Quit must finish the pass and log `quitting`: {lines:#?}"
        );
        assert!(
            !app_quit_logged(&lines, "cancelled"),
            "removing the prompting window during Quit must not log `cancelled`: {lines:#?}"
        );
        // Drain GPUI's simulated prompt while `app` is still alive so its
        // detached task does not wake after `app` is dropped at teardown.
        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// 16b. During `CloseWindow`, removing a window while its native/default test
// prompt is open ends the close pass with `already closed` and clears the
// in-progress state so a later `Quit` on another window runs.
#[gpui::test]
fn close_window_prompt_stops_waiting_when_window_is_removed(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        setup_shell(&mut app);
        let first = open_window(&mut app);
        let second = open_window(&mut app);
        let first_handle = AnyWindowHandle::from(first);
        let second_handle = AnyWindowHandle::from(second);

        let calls: Rc<RefCell<Vec<(AnyWindowHandle, CloseIntent)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let calls_in_guard = calls.clone();
        app.update(move |cx| {
            let ext = Extension::new(cx, "notes").unwrap();
            ext.on_close_request(cx, move |intent, window, _| {
                let handle = window.window_handle();
                calls_in_guard.borrow_mut().push((handle, intent));
                if handle == first_handle {
                    CloseDecision::prompt(save_prompt(), |answer, _, _| match answer {
                        Some(1) => CloseOutcome::Allow,
                        _ => CloseOutcome::Cancel,
                    })
                } else {
                    CloseDecision::Allow
                }
            });
        });

        let close_mark = trace.mark();
        request_close(first, &app);
        assert!(app.has_pending_prompt());
        assert_eq!(
            *calls.borrow(),
            vec![(first_handle, CloseIntent::CloseWindow)]
        );

        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();

        let close_lines = trace.since(close_mark);
        assert!(
            window_close_logged(&close_lines, "already closed"),
            "removing the prompting window must end the close pass with `already closed`: {close_lines:#?}"
        );

        activate(second, &mut app);
        let quit_mark = trace.mark();
        dispatch_quit(&app);
        let quit_lines = trace.since(quit_mark);
        assert!(
            !app_quit_logged_info(&quit_lines, "a close is in progress"),
            "a later Quit must not be refused with `a close is in progress`: {quit_lines:#?}"
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                (first_handle, CloseIntent::CloseWindow),
                (second_handle, CloseIntent::Quit),
            ],
            "a later Quit must run guards on the remaining window: {quit_lines:#?}"
        );
        assert!(
            app_quit_logged(&quit_lines, "quitting"),
            "a later Quit on another window must run and log `quitting`: {quit_lines:#?}"
        );
        assert!(
            !app_quit_logged(&quit_lines, "cancelled"),
            "a later allowed Quit must not log `cancelled`: {quit_lines:#?}"
        );
        // Drain GPUI's simulated prompt while `app` is still alive so its
        // detached task does not wake after `app` is dropped at teardown.
        app.simulate_prompt_answer("Cancel");
        app.run_until_parked();
        assert!(!app.has_pending_prompt());
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
