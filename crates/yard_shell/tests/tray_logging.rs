//! Lifecycle trace and diagnostic log tests for `yard_shell/tray`.
//!
//! Installs one process-wide tracing subscriber and diagnostic logger; its
//! single test runs alone in this binary.

use gpui::{AnyWindowHandle, MenuItem, SystemMenuType, TestAppContext, VisualTestContext};
use image::{ImageFormat, Rgba, RgbaImage};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Cursor,
    rc::Rc,
    sync::Mutex,
};
use yard_shell::{self as shell, Extension, Tray, TrayBackend, TrayError, TrayIcon};

gpui::actions!(notes, [NewNote]);

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
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured {
    next_id: 1,
    spans: BTreeMap::new(),
    step_span_ids: Vec::new(),
    step_events: Vec::new(),
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
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
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
        .filter(|span| span.name.starts_with("tray-"))
        .collect();
    let events = std::mem::take(&mut captured.step_events);
    let diagnostics = std::mem::take(&mut *DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()));
    (spans, events, diagnostics)
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

fn assert_tray_span(
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

fn event_dispatched_to_target(event: &EventRecord, target: &str) -> bool {
    event.level == tracing::Level::DEBUG
        && event.message == "dispatched"
        && event.fields.get("target").map(String::as_str) == Some(target)
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("encoding test PNG must succeed");
    bytes
}

fn open_shell_window(cx: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

#[gpui::test]
fn tray_operations_log_groups_fields_and_levels(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let app_quits = Rc::new(Cell::new(0usize));
    let window_hits: Rc<RefCell<Vec<AnyWindowHandle>>> = Rc::new(RefCell::new(Vec::new()));
    let activations = Rc::new(Cell::new(0usize));

    let (notes, other) = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let app_quits = app_quits.clone();
        cx.on_action(move |_: &shell::Quit, _| {
            app_quits.set(app_quits.get() + 1);
        });
        let notes = Extension::new(cx, "notes").unwrap();
        let window_hits = window_hits.clone();
        notes.on_action::<NewNote>(cx, move |_, window, _| {
            window_hits.borrow_mut().push(window.window_handle());
        });
        let other = Extension::new(cx, "other").unwrap();
        (notes, other)
    });
    let icon = png_bytes(16, 16);
    let _ = take_step();

    // 1. Initial set: tray-set{extension="notes", items=2} with debug "shown".
    cx.update(|cx| {
        notes.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![
                MenuItem::action("Quit", shell::Quit),
                MenuItem::action("New Note", NewNote),
            ]),
            cx,
        )
    })
    .unwrap();
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-set", "notes");
    assert_eq!(
        span.fields.get("items").map(String::as_str),
        Some("2"),
        "expected items=2 on tray-set span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "shown"),
        "expected DEBUG `shown` inside tray-set, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 2. Update with an unsupported MenuItem::os_submenu: tray-set{extension="notes"} with
    //    an INFO line for the omitted item and DEBUG "updated".
    cx.update(|cx| {
        notes.set_tray(
            Tray::new(TrayIcon::png(&icon))
                .tooltip("Updated")
                .menu(vec![
                    MenuItem::action("Quit", shell::Quit),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::action("New Note", NewNote),
                ]),
            cx,
        )
    })
    .unwrap();
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-set", "notes");
    assert!(
        span.fields.contains_key("items"),
        "expected items field on updated tray-set span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::INFO && !e.message.is_empty()),
        "expected INFO line when omitting MenuItem::os_submenu inside tray-set, got {in_span:#?}"
    );
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "updated"),
        "expected DEBUG `updated` inside tray-set, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 3. Refusal AlreadyOwned: info "refused" with reason.
    assert_eq!(
        cx.update(|cx| other.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
        Err(TrayError::AlreadyOwned)
    );
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-set", "other");
    assert!(
        span.fields.contains_key("items"),
        "expected items field on tray-set span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::INFO
                && e.message == "refused"
                && e.fields.get("reason").is_some_and(|r| !r.is_empty())
        }),
        "expected INFO `refused` with reason on AlreadyOwned, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 4. Refusal InvalidIcon: info "refused" with reason.
    assert_eq!(
        cx.update(|cx| notes.set_tray(Tray::new(TrayIcon::png(b"not a png")), cx)),
        Err(TrayError::InvalidIcon)
    );
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-set", "notes");
    assert!(
        span.fields.contains_key("items"),
        "expected items field on tray-set span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span.iter().any(|e| {
            e.level == tracing::Level::INFO
                && e.message == "refused"
                && e.fields.get("reason").is_some_and(|r| !r.is_empty())
        }),
        "expected INFO `refused` with reason on InvalidIcon, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 5. Unavailable and Failed: WARN inside tray-set + one log::warn!, no ERROR.
    for error in [TrayError::Unavailable, TrayError::Failed] {
        backend.fail_next(error);
        assert_eq!(
            cx.update(|cx| notes.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
            Err(error)
        );
        let (spans, events, diagnostics) = take_step();
        let span = assert_tray_span(&spans, "tray-set", "notes");
        let in_span = events_in_span(&events, span.id);
        assert!(
            in_span.iter().any(|e| e.level == tracing::Level::WARN),
            "expected WARN inside tray-set on {error:?}, got {in_span:#?}"
        );
        assert!(
            !events.iter().any(|e| e.level == tracing::Level::ERROR),
            "{error:?} must not emit tracing ERROR, got {events:#?}"
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
            "{error:?} must emit one log::warn!, got {diagnostics:#?}"
        );
        assert!(
            diag_errors.is_empty(),
            "{error:?} must not emit log::error!, got {diagnostics:#?}"
        );
    }

    // 6. Activate with no handler: tray-activate{extension="notes"} with debug "no handler".
    backend.simulate_activate();
    cx.run_until_parked();
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-activate", "notes");
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "no handler"),
        "expected DEBUG `no handler` inside tray-activate, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 7. Activate with handler: tray-activate{extension="notes"} with debug "handled".
    cx.update(|cx| {
        let activations = activations.clone();
        notes.on_tray_activate(cx, move |_| {
            activations.set(activations.get() + 1);
        });
    });
    let (_, first_reg_events, first_reg_diag) = take_step();
    assert!(
        !first_reg_events
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG),
        "initial on_tray_activate must not log a replacement debug line, got {first_reg_events:#?}"
    );
    assert_no_warn_or_error(&first_reg_events, &first_reg_diag);

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(activations.get(), 1);
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-activate", "notes");
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "handled"),
        "expected DEBUG `handled` inside tray-activate, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 7b. Replacing on_tray_activate logs a debug line.
    let replaced_activations = Rc::new(Cell::new(0usize));
    cx.update(|cx| {
        let replaced_activations = replaced_activations.clone();
        notes.on_tray_activate(cx, move |_| {
            replaced_activations.set(replaced_activations.get() + 1);
        });
    });
    let (_, replace_events, replace_diag) = take_step();
    assert!(
        replace_events
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && !e.message.is_empty()),
        "replacing on_tray_activate must log a DEBUG line, got {replace_events:#?}"
    );
    assert_no_warn_or_error(&replace_events, &replace_diag);

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(activations.get(), 1);
    assert_eq!(replaced_activations.get(), 1);
    let _ = take_step();

    // 8. Menu action with no window: tray-menu-action{extension="notes", action="yard::Quit"}
    //    with debug "dispatched" and target="application".
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(app_quits.get(), 1);
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-menu-action", "notes");
    assert_eq!(
        span.fields.get("action").map(String::as_str),
        Some("yard::Quit"),
        "expected action='yard::Quit' on tray-menu-action span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| event_dispatched_to_target(e, "application")),
        "expected DEBUG `dispatched` with target=`application` inside tray-menu-action, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 9a. Menu action with open windows never activated: target="first window".
    let handle_a: AnyWindowHandle = open_shell_window(cx).into();
    let handle_b: AnyWindowHandle = open_shell_window(cx).into();
    assert_ne!(handle_a, handle_b);
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![handle_a, handle_b]);
    let _ = take_step();

    backend.simulate_menu(&[1]);
    cx.run_until_parked();
    assert_eq!(*window_hits.borrow(), vec![handle_a]);
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-menu-action", "notes");
    assert_eq!(
        span.fields.get("action").map(String::as_str),
        Some("notes::NewNote"),
        "expected action='notes::NewNote' on tray-menu-action span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| event_dispatched_to_target(e, "first window")),
        "expected DEBUG `dispatched` with target=`first window` inside tray-menu-action, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 9b. Menu action with an active window: target="window".
    handle_a
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(handle_a));
    let _ = take_step();

    backend.simulate_menu(&[1]);
    cx.run_until_parked();
    assert_eq!(*window_hits.borrow(), vec![handle_a, handle_a]);
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-menu-action", "notes");
    assert_eq!(
        span.fields.get("action").map(String::as_str),
        Some("notes::NewNote"),
        "expected action='notes::NewNote' on tray-menu-action span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| event_dispatched_to_target(e, "window")),
        "expected DEBUG `dispatched` with target=`window` inside tray-menu-action, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 9c. Activate B after A, then deactivate so no window is active: B receives it with
    //     target="last active window".
    handle_b
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(handle_b));
    let mut visual_b = VisualTestContext::from_window(handle_b, cx);
    visual_b.deactivate_window();
    drop(visual_b);
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![handle_a, handle_b]);
    let _ = take_step();

    backend.simulate_menu(&[1]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_a, handle_a, handle_b],
        "with no active window, the most recently active window (B) must receive the action"
    );
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-menu-action", "notes");
    assert_eq!(
        span.fields.get("action").map(String::as_str),
        Some("notes::NewNote"),
        "expected action='notes::NewNote' on tray-menu-action span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| event_dispatched_to_target(e, "last active window")),
        "expected DEBUG `dispatched` with target=`last active window` inside tray-menu-action, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 9d. Close B; with no window active, A receives the action as the last active window
    //     still open, with target="last active window".
    handle_b
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![handle_a]);
    let _ = take_step();

    backend.simulate_menu(&[1]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_a, handle_a, handle_b, handle_a],
        "after closing B, A must receive the action as the last active window still open"
    );
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-menu-action", "notes");
    assert_eq!(
        span.fields.get("action").map(String::as_str),
        Some("notes::NewNote"),
        "expected action='notes::NewNote' on tray-menu-action span: {span:#?}"
    );
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| event_dispatched_to_target(e, "last active window")),
        "expected DEBUG `dispatched` with target=`last active window` after closing B, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    // 10. Remove: tray-remove{extension="notes"} with debug "removed".
    cx.update(|cx| notes.remove_tray(cx));
    let (spans, events, diagnostics) = take_step();
    let span = assert_tray_span(&spans, "tray-remove", "notes");
    let in_span = events_in_span(&events, span.id);
    assert!(
        in_span
            .iter()
            .any(|e| e.level == tracing::Level::DEBUG && e.message == "removed"),
        "expected DEBUG `removed` inside tray-remove, got {in_span:#?}"
    );
    assert_no_warn_or_error(&events, &diagnostics);

    ENTERED.with(|stack| {
        assert!(
            stack.borrow().is_empty(),
            "every entered span on the main thread must exit cleanly"
        );
    });
}
