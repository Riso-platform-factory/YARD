#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs.
const PROBE: &str = "titles_probe";

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
        if !target.starts_with("riso_shell") && target != PROBE {
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

struct TitledCard {
    title: gpui::SharedString,
}

impl gpui::Render for TitledCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

impl shell::Titled for TitledCard {
    fn title(&self, _: &gpui::App) -> gpui::SharedString {
        self.title.clone()
    }
}

struct PlainCard;

impl gpui::Render for PlainCard {
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

// Alone in this binary: every test that reaches these log lines shares this
// one subscriber, so no callsite caches "nobody listens" out from under it.
#[gpui::test]
fn titles_log_their_register_group_each_read_and_one_wrong_view_error(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let _host = app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| TitledCard {
                    title: format!("{node}-title").into(),
                })
                .into()
            });
            host.register_title::<TitledCard>(cx, "tour.card");
            // The extension bug the contract names: tour.wrong's views are
            // PlainCard, but its title is registered for TitledCard.
            host.register(cx, "tour.wrong", &[], move |_, _, _, cx| {
                cx.new(|_| PlainCard).into()
            });
            host.register_title::<TitledCard>(cx, "tour.wrong");
            host.register(cx, "tour.plain", &[], move |_, _, _, cx| {
                cx.new(|_| PlainCard).into()
            });
            // Refused: another extension's component, and a second time.
            host.register_title::<TitledCard>(cx, "guest.card");
            host.register_title::<TitledCard>(cx, "tour.card");
            host.place_root(cx, "tabs", "riso.tabs");
            host.place(cx, "alpha", "tour.card", "tabs", "children");
            host.place(cx, "mismatch", "tour.wrong", "tabs", "children");
            host.place(cx, "gamma", "tour.plain", "tabs", "children");
            host
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        for _ in 0..3 {
            draw(window, &mut app);
        }
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    // A component says it gives titles inside its own group.
    for component in ["tour.card", "tour.wrong"] {
        assert!(
            lines.iter().any(|line| line
                == &format!("title-register component={component}: DEBUG message=gives titles")),
            "{component}'s group logs `gives titles`: {lines:#?}"
        );
    }
    // A refusal logs an ERROR in its group and gives no titles.
    assert!(
        lines.iter().any(
            |line| line.starts_with("title-register component=guest.card:")
                && line.contains("ERROR")
        ),
        "the namespace refusal: {lines:#?}"
    );
    assert!(
        !lines.iter().any(
            |line| line.starts_with("title-register component=guest.card:")
                && line.contains("gives titles")
        ),
        "guest.card gives no titles: {lines:#?}"
    );
    assert!(
        lines.iter().any(
            |line| line.starts_with("title-register component=tour.card:")
                && line.contains("ERROR")
        ),
        "the second-time refusal: {lines:#?}"
    );
    // A tab reads a title at trace, on every frame.
    let title_reads = lines
        .iter()
        .filter(|line| {
            line.contains("TRACE") && line.contains("tab title") && line.contains("alpha")
        })
        .count();
    assert!(
        title_reads >= 3,
        "a `tab title` trace for alpha on each of three frames: {lines:#?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("tab title") && !line.contains("TRACE")),
        "a `tab title` read above trace: {lines:#?}"
    );
    // A view of another type is one diagnostic ERROR though it draws every
    // frame, and the lifecycle trace has it each frame.
    let errors: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("ERROR") && line.contains("view of node mismatch"))
        .collect();
    assert_eq!(errors.len(), 1, "{lines:#?}");
    assert!(
        errors[0].contains("view of node mismatch is not the type that gives its title"),
        "the diagnostic says the view is not the type: {errors:?}"
    );
    let traces = lines
        .iter()
        .filter(|line| line.contains("TRACE") && line.contains("mismatch"))
        .count();
    assert!(
        traces >= 3,
        "the wrong view is logged at trace each frame: {lines:#?}"
    );
}
