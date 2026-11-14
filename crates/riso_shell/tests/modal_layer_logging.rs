use gpui::{AppContext, InteractiveElement, IntoElement, ParentElement, Styled, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
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
            tracing::error!(target: "modal_layer_diagnostic", "{}", record.args());
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

/// The surface every test modal draws: a fixed-size box that is its own
/// focusable element, holding a close button that emits `DismissEvent` on the
/// modal's view. `<selector>-CLOSE` hugs the close button.
fn surface<V: gpui::EventEmitter<gpui::DismissEvent> + 'static>(
    selector: &'static str,
    focus: &gpui::FocusHandle,
    view: gpui::Entity<V>,
) -> gpui::Div {
    gpui::div()
        .debug_selector(move || selector.to_string())
        .track_focus(focus)
        .w(gpui::px(240.))
        .h(gpui::px(120.))
        .child(
            gpui::div()
                .debug_selector(move || format!("{selector}-CLOSE"))
                .child(
                    riso_ui::Button::new("close", "close")
                        .size(riso_ui::ButtonSize::Compact)
                        .on_click(move |_, cx| {
                            view.update(cx, |_, cx| cx.emit(gpui::DismissEvent))
                        }),
                ),
        )
}

struct TestModal {
    focus: gpui::FocusHandle,
}

impl TestModal {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl gpui::EventEmitter<gpui::DismissEvent> for TestModal {}
impl gpui::Focusable for TestModal {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}
impl riso_ui::ModalView for TestModal {}
impl gpui::Render for TestModal {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        surface("MODAL-TEST", &self.focus, cx.entity())
    }
}

struct OtherModal {
    focus: gpui::FocusHandle,
}

impl OtherModal {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl gpui::EventEmitter<gpui::DismissEvent> for OtherModal {}
impl gpui::Focusable for OtherModal {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}
impl riso_ui::ModalView for OtherModal {}
impl gpui::Render for OtherModal {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        surface("MODAL-OTHER", &self.focus, cx.entity())
    }
}

struct DimmedModal {
    focus: gpui::FocusHandle,
}

impl DimmedModal {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl gpui::EventEmitter<gpui::DismissEvent> for DimmedModal {}
impl gpui::Focusable for DimmedModal {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}
impl riso_ui::ModalView for DimmedModal {
    fn dims_background(&self) -> bool {
        true
    }
}
impl gpui::Render for DimmedModal {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        surface("MODAL-DIMMED", &self.focus, cx.entity())
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Shows a modal of type `V` through the extension.
fn toggle<V: riso_ui::ModalView>(
    ext: &riso_shell::Extension,
    window: gpui::WindowHandle<riso_shell::Shell>,
    app: &mut TestAppContext,
    build: impl FnOnce(&mut gpui::Window, &mut gpui::Context<V>) -> V,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            ext.toggle_modal::<V>(window, cx, build)
        })
        .unwrap();
}

/// Closes the open modal through the extension.
fn hide(
    ext: &riso_shell::Extension,
    window: gpui::WindowHandle<riso_shell::Shell>,
    app: &mut TestAppContext,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| ext.hide_modal(window, cx))
        .unwrap();
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
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

/// Every `modal-*` group ever created.
fn modal_spans(trace: &Trace) -> usize {
    spans_named(trace, "modal-").len()
}

/// A trace line about the modal layer's draw: it names the shown modal, over
/// every crate's lines whatever the target.
fn is_modal_draw(line: &str) -> bool {
    let line = line.replace('"', "");
    line.contains(": TRACE")
        && line
            .split_whitespace()
            .any(|token| token.starts_with("modal="))
}

/// The `(modal, dimmed)` of the one trace line a draw may log about the modal
/// layer, or `None` while no modal is drawn.
fn drawn_modal(lines: &[String]) -> Option<(String, bool)> {
    let about: Vec<&String> = lines.iter().filter(|line| is_modal_draw(line)).collect();
    if about.is_empty() {
        return None;
    }
    assert_eq!(
        about.len(),
        1,
        "a draw must log exactly one trace line about the modal layer, in {lines:#?}"
    );
    let line = about[0].replace('"', "");
    let modal = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("modal="))
        .expect("the draw line names the modal")
        .to_string();
    let dimmed = line
        .split_whitespace()
        .find_map(|token| {
            let (name, value) = token.split_once('=')?;
            if name.starts_with("dim") {
                value.parse::<bool>().ok()
            } else {
                None
            }
        })
        .expect("the draw line says whether the background is dimmed");
    Some((modal, dimmed))
}

/// The innermost group enclosing `line`, or "" when no group was open.
fn group_of(line: &str) -> &str {
    line.split_once(": ").map(|(group, _)| group).unwrap_or("")
}

/// The `keys` of the shell's other bindings, before `escape` joined them.
#[cfg(target_os = "macos")]
const SHELL_KEYS: usize = 9;
#[cfg(not(target_os = "macos"))]
const SHELL_KEYS: usize = 7;

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn the_modal_layer_logs_shows_hides_reasons_focus_and_draws(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        {
            // The startup binds the default keys in `app-init`, before extensions run.
            let _group = tracing::info_span!("app-init").entered();
            app.update(riso_shell::bind_default_keys);
        }
        app.update(riso_ui::init);
        let ext = app.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
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
            cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
        draw(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // A focused card gives every close a previous focus to restore.
        click(&mut visual, "CARD-a");
        draw(window, &mut app);
        let open_test = |app: &mut TestAppContext| {
            toggle::<TestModal>(&ext, window, app, |_, cx| {
                tracing::debug!("test modal built");
                TestModal::new(cx)
            });
        };
        open_test(&mut app);
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(
            drawn_modal(&drawn),
            Some(("TestModal".to_string(), false)),
            "one draw line names the modal and that it does not dim"
        );
        // The modal's own close button emits the `DismissEvent`.
        click(&mut visual, "MODAL-TEST-CLOSE");
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None, "closed: no draw line");
        open_test(&mut app);
        draw(window, &mut app);
        visual.simulate_keystrokes("escape");
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None);
        open_test(&mut app);
        draw(window, &mut app);
        visual.simulate_click(
            gpui::point(gpui::px(10.), gpui::px(560.)),
            gpui::Modifiers::default(),
        );
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None);
        open_test(&mut app);
        draw(window, &mut app);
        hide(&ext, window, &mut app);
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None);
        // A toggle that closes shows nothing.
        open_test(&mut app);
        draw(window, &mut app);
        let shows = spans_named(&trace, "modal-show").len();
        open_test(&mut app);
        assert_eq!(
            spans_named(&trace, "modal-show").len(),
            shows,
            "a toggle that closes opens no `modal-show` group"
        );
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None);
        // A different type replaces the open modal.
        open_test(&mut app);
        draw(window, &mut app);
        toggle::<OtherModal>(&ext, window, &mut app, |_, cx| {
            tracing::debug!("other modal built");
            OtherModal::new(cx)
        });
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), Some(("OtherModal".to_string(), false)));
        visual.simulate_keystrokes("escape");
        draw(window, &mut app);
        // A modal that asks dims the background over the whole window.
        toggle::<DimmedModal>(&ext, window, &mut app, |_, cx| {
            tracing::debug!("dimmed modal built");
            DimmedModal::new(cx)
        });
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(
            drawn_modal(&drawn),
            Some(("DimmedModal".to_string(), true)),
            "the draw line reports the dimmed background"
        );
        visual.simulate_keystrokes("escape");
        let drawn = lines_of(&trace, || draw(window, &mut app));
        assert_eq!(drawn_modal(&drawn), None);
        // `hide_modal` and escape with no modal log nothing: no group opens
        // and no captured line at any level mentions the layer's work.
        let before = modal_spans(&trace);
        let quiet = lines_of(&trace, || {
            hide(&ext, window, &mut app);
            visual.simulate_keystrokes("escape");
        });
        assert_eq!(
            modal_spans(&trace),
            before,
            "hide_modal and escape with no modal open no `modal-*` group"
        );
        assert!(
            quiet.iter().all(|line| !line.contains("modal")),
            "no captured line mentions `modal`: {quiet:#?}"
        );
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let spans: Vec<String> = captured
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .collect();
    // `keys-bind` counts one more key: `escape` for `riso::Dismiss` in `ModalLayer`.
    let keys_bind = format!("keys-bind count={}", SHELL_KEYS + 1);
    assert_eq!(
        spans.iter().filter(|span| **span == keys_bind).count(),
        1,
        "one `{keys_bind}` group in {spans:#?}"
    );
    let inside: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| group_of(line) == keys_bind)
        .collect();
    assert_eq!(
        inside.len(),
        SHELL_KEYS + 1,
        "one line per bound key inside `{keys_bind}`: {inside:#?}"
    );
    assert_eq!(
        inside
            .iter()
            .filter(|line| {
                line.contains(": DEBUG")
                    && line.contains("keys=escape")
                    && line.contains("action=riso::Dismiss")
                    && line.contains("context=ModalLayer")
                    && line.contains("message=bound")
            })
            .count(),
        1,
        "`{keys_bind}` must name `escape` for `riso::Dismiss` in `ModalLayer` once: {inside:#?}"
    );
    // `modal-show{modal}` is logged per shown modal, and the extension's own
    // builder lines are inside it.
    let shows_of = |modal: &str| {
        spans
            .iter()
            .filter(|span| span.as_str() == format!("modal-show modal={modal}"))
            .count()
    };
    assert_eq!(shows_of("TestModal"), 6, "TestModal was shown six times");
    assert_eq!(shows_of("OtherModal"), 1);
    assert_eq!(shows_of("DimmedModal"), 1);
    for (modal, built) in [
        ("TestModal", "test modal built"),
        ("OtherModal", "other modal built"),
        ("DimmedModal", "dimmed modal built"),
    ] {
        let group = format!("modal-show modal={modal}");
        assert!(
            lines.iter().any(|line| {
                group_of(line) == group && line.contains(": DEBUG") && line.contains(built)
            }),
            "the extension's line {built:?} is inside `{group}`: {lines:#?}"
        );
    }
    // Every closing reason gets its `modal-hide{modal, reason}` group, with
    // `focus restored` at DEBUG inside while the focus goes back.
    for (modal, reason) in [
        ("TestModal", "dismiss-event"),
        ("TestModal", "escape"),
        ("TestModal", "click-outside"),
        ("TestModal", "hide-modal"),
        ("TestModal", "toggle"),
        ("TestModal", "replaced"),
        ("OtherModal", "escape"),
        ("DimmedModal", "escape"),
    ] {
        let group = format!("modal-hide modal={modal} reason={reason}");
        assert_eq!(
            spans.iter().filter(|span| **span == group).count(),
            1,
            "one `{group}` group in {spans:#?}"
        );
        assert!(
            lines.iter().any(|line| {
                group_of(line) == group
                    && line.contains(": DEBUG")
                    && line.contains("focus restored")
            }),
            "`focus restored` at DEBUG inside `{group}`: {lines:#?}"
        );
    }
    assert_eq!(
        spans
            .iter()
            .filter(|span| span.starts_with("modal-hide"))
            .count(),
        8,
        "eight hides in total in {spans:#?}"
    );
    // A replace closes the open modal before it shows the new one.
    let hid = lines.iter().position(|line| {
        group_of(line) == "modal-hide modal=TestModal reason=replaced"
            && line.contains("focus restored")
    });
    let shown = lines.iter().position(|line| {
        group_of(line) == "modal-show modal=OtherModal" && line.contains("other modal built")
    });
    assert!(
        hid.is_some() && shown.is_some() && hid < shown,
        "the replaced modal hides before the new one shows: {lines:#?}"
    );
    assert_eq!(
        lines.iter().filter(|line| line.contains(": ERROR")).count(),
        0,
        "nothing at ERROR in {lines:#?}"
    );
}
