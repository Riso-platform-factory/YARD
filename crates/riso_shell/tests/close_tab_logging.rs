use gpui::{AppContext, IntoElement, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs.
const PROBE: &str = "close_tab_probe";

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
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && target != PROBE
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
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

// The one process-wide diagnostic logger this file installs once; alone here,
// it cannot collide with another test's logger.
struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            _ => tracing::info!(target: PROBE, "{}", record.args()),
        }
    }
    fn flush(&self) {}
}

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}

fn mark(trace: &Trace) -> usize {
    trace.0.lock().unwrap().lines.len()
}

// The lines captured since a mark, with quotes stripped.
fn lines_since(trace: &Trace, mark: usize) -> Vec<String> {
    trace.0.lock().unwrap().lines[mark..]
        .iter()
        .map(|line| line.replace('"', ""))
        .collect()
}

// Alone in this binary: every call reaching these log lines shares this one
// subscriber, so no callsite caches "nobody listens" out from under it.
#[gpui::test]
fn closing_a_tab_logs_tab_close_then_the_remove_op_and_each_dropped_view(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
            host.register(cx, "tour.required", &[], |_, _, _, cx| {
                cx.new(|_| Card).into()
            });
            host.require(cx, "tour.required");
            host.place_root(cx, "tabs", "riso.tabs");
            host.place(cx, "alpha", "tour.card", "tabs", "children");
            host.place(cx, "beta", "tour.card", "tabs", "children");
            // `pair` holds two cards, so closing it drops three views.
            host.place(cx, "pair", "riso.row", "tabs", "children");
            host.place(cx, "sub-one", "tour.card", "pair", "children");
            host.place(cx, "sub-two", "tour.card", "pair", "children");
            host.place(cx, "pinned", "tour.required", "tabs", "children");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        // Resting the pointer on a tab draws its close button: nothing beyond
        // the tab's own trace lines is logged.
        let before = mark(&trace);
        let beta_tab = visual
            .debug_bounds("TAB-beta")
            .expect("beta's tab draws")
            .center();
        visual.simulate_mouse_move(beta_tab, None, gpui::Modifiers::default());
        draw(window, &mut app);
        let lines = lines_since(&trace, before);
        assert!(
            lines.is_empty(),
            "drawing the button adds nothing: {lines:#?}"
        );

        // A middle click on the required node's tab does nothing, not even a
        // `tab close` or a refused remove.
        let before = mark(&trace);
        let pinned_tab = visual
            .debug_bounds("TAB-pinned")
            .expect("the required tab draws")
            .center();
        visual.simulate_mouse_down(
            pinned_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            pinned_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        draw(window, &mut app);
        let lines = lines_since(&trace, before);
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("tab close") || line.starts_with("layout-op")),
            "a middle click on a required node's tab does nothing: {lines:#?}"
        );

        // A middle click closes beta: `tab close`, then the remove's group.
        let before = mark(&trace);
        visual.simulate_mouse_down(
            beta_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            beta_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        let lines = lines_since(&trace, before);
        let close = lines.iter().position(|line| {
            line.contains("DEBUG")
                && line.contains("message=tab close")
                && line.contains("node=beta")
        });
        let remove = lines
            .iter()
            .position(|line| line.starts_with("layout-op op=remove node=beta extension=riso:"));
        let Some(close) = close else {
            panic!("the `tab close` debug is missing: {lines:#?}");
        };
        let Some(remove) = remove else {
            panic!("the remove's group is missing: {lines:#?}");
        };
        assert!(
            close < remove,
            "`tab close` comes before the remove: {lines:#?}"
        );
        for expected in [
            "layout-op op=remove node=beta extension=riso: DEBUG message=node removed node=beta removed=1",
            "layout-op op=remove node=beta extension=riso: DEBUG message=view dropped node=beta",
        ] {
            assert!(
                lines.iter().any(|line| line.as_str() == expected),
                "missing {expected:?} in {lines:#?}"
            );
        }

        // Closing `pair` drops every view under it.
        let before = mark(&trace);
        let pair_tab = visual
            .debug_bounds("TAB-pair")
            .expect("pair's tab draws")
            .center();
        visual.simulate_mouse_down(
            pair_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        visual.simulate_mouse_up(
            pair_tab,
            gpui::MouseButton::Middle,
            gpui::Modifiers::default(),
        );
        let lines = lines_since(&trace, before);
        let close = lines.iter().position(|line| {
            line.contains("DEBUG")
                && line.contains("message=tab close")
                && line.contains("node=pair")
        });
        let remove = lines
            .iter()
            .position(|line| line.starts_with("layout-op op=remove node=pair extension=riso:"));
        let Some(close) = close else {
            panic!("the `tab close` debug is missing: {lines:#?}");
        };
        let Some(remove) = remove else {
            panic!("the remove's group is missing: {lines:#?}");
        };
        assert!(
            close < remove,
            "`tab close` comes before the remove: {lines:#?}"
        );
        for expected in [
            "layout-op op=remove node=pair extension=riso: DEBUG message=node removed node=pair removed=3",
            "layout-op op=remove node=pair extension=riso: DEBUG message=view dropped node=pair",
            "layout-op op=remove node=pair extension=riso: DEBUG message=view dropped node=sub-one",
            "layout-op op=remove node=pair extension=riso: DEBUG message=view dropped node=sub-two",
        ] {
            assert!(
                lines.iter().any(|line| line.as_str() == expected),
                "missing {expected:?} in {lines:#?}"
            );
        }

        assert!(trace.0.lock().unwrap().entered.is_empty());
    });
}
