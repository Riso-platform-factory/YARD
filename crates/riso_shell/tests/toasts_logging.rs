#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_ui::dev::{Toast, ToastKind};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

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
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "toasts_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::dev::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Shows `toast` through the extension.
fn show_toast(
    ext: &riso_shell::dev::Extension,
    window: gpui::WindowHandle<riso_shell::dev::Shell>,
    app: &mut TestAppContext,
    toast: Toast,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| ext.show_toast(toast, window, cx))
        .unwrap();
}

/// Closes the shown toast through the extension.
fn hide_toast(
    ext: &riso_shell::dev::Extension,
    window: gpui::WindowHandle<riso_shell::dev::Shell>,
    app: &mut TestAppContext,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| ext.hide_toast(window, cx))
        .unwrap();
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

/// The bounds of the element with `selector` in the last draw.
fn bounds_of(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Bounds<gpui::Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"))
}

/// The lines captured while `run` executes.
fn lines_of(trace: &Trace, run: impl FnOnce()) -> Vec<String> {
    let before = trace.0.lock().unwrap().lines.len();
    run();
    trace.0.lock().unwrap().lines[before..].to_vec()
}

/// The spans ever created whose name starts with `prefix`, quotes stripped for
/// field-value checks.
fn spans_named(trace: &Trace, prefix: &str) -> Vec<String> {
    trace
        .0
        .lock()
        .unwrap()
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .filter(|span| span.starts_with(prefix))
        .collect()
}

/// Every `toast-*` group ever created.
fn toast_spans(trace: &Trace) -> usize {
    spans_named(trace, "toast-").len()
}

/// The toast kind a `name=value` token names, if it does, lowercase.
fn token_kind(token: &str) -> Option<String> {
    match token.split_once('=') {
        Some(("toast" | "kind" | "severity", kind))
            if ["info", "warning", "error"].contains(&kind.to_ascii_lowercase().as_str()) =>
        {
            Some(kind.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// A trace line about the toast layer's draw: it names the shown toast's kind,
/// over every crate's lines whatever the target.
fn is_toast_draw(line: &str) -> bool {
    line.contains(": TRACE")
        && line
            .replace('"', "")
            .split_whitespace()
            .any(|token| token_kind(token).is_some())
}

/// The kind named by the one trace line a draw may log about the toast layer,
/// or `None` while no toast is drawn.
fn drawn_toast_kind(lines: &[String]) -> Option<String> {
    let about: Vec<&String> = lines.iter().filter(|line| is_toast_draw(line)).collect();
    if about.is_empty() {
        return None;
    }
    assert_eq!(
        about.len(),
        1,
        "a draw must log exactly one trace line about the toast layer, in {lines:#?}"
    );
    let kind = about[0]
        .replace('"', "")
        .split_whitespace()
        .find_map(token_kind)
        .expect("the draw line names the toast's kind");
    Some(kind)
}

/// The innermost group enclosing `line`, or "" when no group was open.
fn group_of(line: &str) -> &str {
    line.split_once(": ").map(|(group, _)| group).unwrap_or("")
}

/// The integer value of the `name=value` field in `line`, if it is there.
fn field_value(line: &str, name: &str) -> Option<u128> {
    line.split_whitespace().find_map(|token| {
        let (field, value) = token.split_once('=')?;
        (field == name).then(|| value.parse().ok()).flatten()
    })
}

/// Advances the test clock by `duration` and runs everything it unblocks. The
/// first park lets a just-spawned timer register its deadline before the clock
/// moves.
fn advance(app: &mut TestAppContext, duration: Duration) {
    app.run_until_parked();
    app.executor().advance_clock(duration);
    app.run_until_parked();
}

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn the_toast_layer_logs_shows_hides_reasons_pause_resume_and_draws(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        {
            // The startup binds the default keys in `app-init`, before extensions run.
            let _group = tracing::info_span!("app-init").entered();
            app.update(riso_shell::dev::bind_default_keys);
        }
        app.update(riso_ui::init);
        let ext = app.update(|cx| {
            let ext = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            ext.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "a", "tour.card", "root", "children");
            ext
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
        draw(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // A timeout closes the toast on its own.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Info, "gone soon"),
        );
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(
            drawn_toast_kind(&drawn).as_deref(),
            Some("info"),
            "one draw line names the toast's kind"
        );
        advance(&mut app, Duration::from_secs(10));
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_toast_kind(&drawn), None, "closed: no draw line");
        // The close button.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Warning, "close me"),
        );
        draw(window, &mut app);
        click(&mut visual, "TOAST-DISMISS");
        visual.simulate_mouse_move(
            gpui::point(gpui::px(10.), gpui::px(10.)),
            None,
            gpui::Modifiers::default(),
        );
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_toast_kind(&drawn), None);
        // The button: its handler's own lines are inside `toast-action`.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Error, "failed").action("Retry", |_, _| {
                tracing::debug!("retry clicked");
            }),
        );
        draw(window, &mut app);
        click(&mut visual, "TOAST-ACTION");
        visual.simulate_mouse_move(
            gpui::point(gpui::px(10.), gpui::px(10.)),
            None,
            gpui::Modifiers::default(),
        );
        draw(window, &mut app);
        // A middle click.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Info, "middle"),
        );
        draw(window, &mut app);
        let toast = bounds_of(&mut visual, "TOAST");
        visual.simulate_mouse_down(
            toast.center(),
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            toast.center(),
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        visual.simulate_mouse_move(
            gpui::point(gpui::px(10.), gpui::px(10.)),
            None,
            gpui::Modifiers::default(),
        );
        draw(window, &mut app);
        // `hide_toast` from the extension.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Warning, "hidden"),
        );
        draw(window, &mut app);
        hide_toast(&ext, window, &mut app);
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_toast_kind(&drawn), None);
        // A second toast replaces the first.
        show_toast(&ext, window, &mut app, Toast::new(ToastKind::Info, "first"));
        draw(window, &mut app);
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Error, "second"),
        );
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(
            drawn_toast_kind(&drawn).as_deref(),
            Some("error"),
            "the new toast draws"
        );
        advance(&mut app, Duration::from_secs(10));
        draw(window, &mut app);
        // The pointer pauses the timer; what is left is resumed.
        show_toast(&ext, window, &mut app, Toast::new(ToastKind::Info, "pause"));
        draw(window, &mut app);
        advance(&mut app, Duration::from_secs(2));
        draw(window, &mut app);
        let toast = bounds_of(&mut visual, "TOAST");
        visual.simulate_mouse_move(toast.center(), None, gpui::Modifiers::default());
        draw(window, &mut app);
        visual.simulate_mouse_move(
            gpui::point(gpui::px(10.), gpui::px(10.)),
            None,
            gpui::Modifiers::default(),
        );
        draw(window, &mut app);
        hide_toast(&ext, window, &mut app);
        draw(window, &mut app);
        // With less than 0.8 s left, the resumed timer still gets 0.8 s.
        show_toast(
            &ext,
            window,
            &mut app,
            Toast::new(ToastKind::Warning, "nearly gone"),
        );
        draw(window, &mut app);
        advance(
            &mut app,
            Duration::from_secs(10) - Duration::from_millis(500),
        );
        draw(window, &mut app);
        let toast = bounds_of(&mut visual, "TOAST");
        visual.simulate_mouse_move(toast.center(), None, gpui::Modifiers::default());
        draw(window, &mut app);
        visual.simulate_mouse_move(
            gpui::point(gpui::px(10.), gpui::px(10.)),
            None,
            gpui::Modifiers::default(),
        );
        draw(window, &mut app);
        advance(&mut app, Duration::from_millis(800));
        draw(window, &mut app);
        // `hide_toast` with no toast logs nothing: no group opens and no
        // captured line at any level mentions the layer's work.
        let before = toast_spans(&trace);
        let quiet = lines_of(&trace, || hide_toast(&ext, window, &mut app));
        assert_eq!(
            toast_spans(&trace),
            before,
            "hide_toast with no toast opens no `toast-*` group"
        );
        assert!(
            quiet.iter().all(|line| !line.contains("toast")),
            "no captured line mentions `toast`: {quiet:#?}"
        );
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    // Span creation order is group-open order.
    let ordered: Vec<String> = captured
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .collect();
    // `toast-show{kind}` is logged per shown toast, with `timer started` at
    // DEBUG and `duration_ms=10000` inside it.
    let shows_of = |kind: &str| {
        ordered
            .iter()
            .filter(|span| span.as_str() == format!("toast-show kind={kind}"))
            .count()
    };
    assert_eq!(shows_of("info"), 4, "info was shown four times");
    assert_eq!(shows_of("warning"), 3);
    assert_eq!(shows_of("error"), 2);
    let started: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| {
            group_of(line).starts_with("toast-show kind=")
                && line.contains(": DEBUG")
                && line.contains("timer started")
                && line.contains("duration_ms=10000")
        })
        .collect();
    assert_eq!(
        started.len(),
        9,
        "every `toast-show` group logs `timer started` with `duration_ms=10000`: {lines:#?}"
    );
    for kind in ["info", "warning", "error"] {
        let group = format!("toast-show kind={kind}");
        assert!(
            started.iter().any(|line| group_of(line) == group),
            "`timer started` inside `{group}`: {lines:#?}"
        );
    }
    // Every closing reason gets its `toast-hide{kind, reason}` group.
    for (kind, reason) in [
        ("info", "timeout"),
        ("warning", "dismiss-button"),
        ("error", "action"),
        ("info", "middle-click"),
        ("warning", "hide-toast"),
        ("info", "replaced"),
        ("error", "timeout"),
        ("info", "hide-toast"),
        ("warning", "timeout"),
    ] {
        let group = format!("toast-hide kind={kind} reason={reason}");
        assert_eq!(
            ordered.iter().filter(|span| **span == group).count(),
            1,
            "one `{group}` group in {ordered:#?}"
        );
    }
    assert_eq!(
        ordered
            .iter()
            .filter(|span| span.starts_with("toast-hide"))
            .count(),
        9,
        "nine hides in total in {ordered:#?}"
    );
    // A replace closes the shown toast before it shows the new one.
    let hid = ordered
        .iter()
        .position(|span| span.as_str() == "toast-hide kind=info reason=replaced")
        .expect("the replaced toast's `toast-hide` group");
    let shown = ordered[hid..]
        .iter()
        .position(|span| span.starts_with("toast-show"))
        .map(|offset| hid + offset);
    assert_eq!(
        shown.map(|index| ordered[index].as_str()),
        Some("toast-show kind=error"),
        "the old toast's `toast-hide` comes before the new one's `toast-show`: {ordered:#?}"
    );
    // The button's group wraps the extension's handler, then `reason=action`.
    let action = ordered
        .iter()
        .position(|span| span.as_str() == "toast-action kind=error label=Retry")
        .expect("one `toast-action kind=error label=Retry` group");
    let hid = ordered
        .iter()
        .position(|span| span.as_str() == "toast-hide kind=error reason=action")
        .expect("`toast-hide` with `reason=action`");
    assert!(
        action < hid,
        "`toast-action` wraps the handler before `toast-hide`: {ordered:#?}"
    );
    assert!(
        lines.iter().any(|line| {
            group_of(line) == "toast-action kind=error label=Retry"
                && line.contains(": DEBUG")
                && line.contains("retry clicked")
        }),
        "the handler's line is inside `toast-action kind=error label=Retry`: {lines:#?}"
    );
    // The pointer pauses and resumes the timer, both at DEBUG with
    // `remaining_ms`; the resumed value is the time that was left, never
    // below 0.8 s.
    let paused: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| line.contains(": DEBUG") && line.contains("timer paused"))
        .collect();
    assert_eq!(
        paused.len(),
        2,
        "two `timer paused` lines at DEBUG: {lines:#?}"
    );
    assert_eq!(
        field_value(paused[0], "remaining_ms"),
        Some(8000),
        "the first pause had 8 s left: {paused:#?}"
    );
    for line in &paused {
        assert!(
            field_value(line, "remaining_ms").is_some(),
            "`timer paused` carries `remaining_ms`: {line}"
        );
    }
    let resumed: Vec<u128> = lines
        .iter()
        .filter(|line| line.contains(": DEBUG") && line.contains("timer resumed"))
        .map(|line| {
            field_value(line, "remaining_ms").expect("`timer resumed` carries `remaining_ms`")
        })
        .collect();
    assert_eq!(
        resumed,
        [8000, 800],
        "the resumed values match the time advanced and are never below 800: {lines:#?}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.contains(": ERROR")).count(),
        0,
        "nothing at ERROR in {lines:#?}"
    );
}
