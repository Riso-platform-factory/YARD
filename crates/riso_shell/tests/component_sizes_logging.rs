#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The target of the bridged diagnostic log records.
const PROBE: &str = "component_sizes_probe";

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
    span_stacks: Vec<Vec<String>>,
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
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        let target = attributes.metadata().target();
        if target.starts_with("riso_shell") || target.starts_with("riso_paths") {
            let mut stack: Vec<String> = captured
                .entered
                .iter()
                .map(|entered| captured.spans[entered].clone())
                .collect();
            stack.push(label.clone());
            captured.span_stacks.push(stack);
        }
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && !target.starts_with("riso_paths")
            && !target.starts_with("riso_ui")
            && target != PROBE
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

struct Diagnostics;

// A record in the diagnostic log is bridged into the trace, to see it inside
// the lifecycle group it belongs to.
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

// A card fills the zone its split gives it and is found as `CARD-<node>`.
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

// Hints of a `min` of `width` by `height` rems, and a `preferred` width of
// `preferred` rems where one is given.
fn hints(width: f32, height: f32, preferred: Option<f32>) -> shell::SizeHints {
    shell::SizeHints {
        min: gpui::size(gpui::rems(width), gpui::rems(height)),
        preferred: gpui::size(preferred.map(gpui::rems), None),
    }
}

// Declaring a component's size hints opens the `size-hints` group, named for
// the component, and logs the declared sizes at debug; a refusal logs an
// ERROR in the group of the component it names; and the split's layout traces
// the children's minimums, in pixels, on every frame.
#[gpui::test]
fn size_hints_opens_its_group_logs_declared_and_each_refusal_an_error(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            // Declared before the component is registered.
            host.size_hints(cx, "tour.wide", hints(20., 6.25, None));
            host.register(cx, "tour.wide", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "wide", "tour.wide", "root", "children");
            host.place(cx, "plain", "tour.card", "root", "children");
            // Two refusals: a second declaration, and a foreign component.
            host.size_hints(cx, "tour.wide", hints(8., 6.25, None));
            host.size_hints(cx, "other.card", hints(5., 6.25, None));
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        window
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                let _ = window.draw(cx);
            })
            .unwrap();
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let stacks: Vec<Vec<String>> = captured
        .span_stacks
        .iter()
        .map(|stack| stack.iter().map(|span| span.replace('"', "")).collect())
        .collect();
    // The group's name, and the component it names.
    assert!(
        stacks.iter().flatten().any(|span| {
            span.starts_with("size-hints")
                && span.contains("component")
                && span.contains("tour.wide")
        }),
        "{stacks:#?}"
    );
    // `declared` inside the group, with the min and the preferred.
    let declared: Vec<&String> = lines
        .iter()
        .filter(|line| {
            line.starts_with("size-hints") && line.contains("DEBUG") && line.contains("declared")
        })
        .collect();
    assert_eq!(declared.len(), 1, "{lines:#?}");
    assert!(
        declared[0].contains("min") && declared[0].contains("preferred"),
        "{}",
        declared[0]
    );
    // The two refusals: an ERROR each, inside a size-hints group.
    let refused: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("size-hints") && line.contains("ERROR"))
        .collect();
    assert_eq!(refused.len(), 2, "{lines:#?}");
    // A split's layout traces the children's minimums, in pixels.
    let minimums: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("TRACE") && line.contains("child minimums"))
        .collect();
    assert!(!minimums.is_empty(), "{lines:#?}");
    for line in &minimums {
        assert!(line.contains("split") && line.contains("root"), "{line}");
        let mins = line.split_once("mins=").map(|(_, rest)| rest).unwrap_or("");
        // `wide`'s 320 px and `plain`'s 80 px at the 16 px font.
        assert!(mins.contains("320") && mins.contains("80"), "{line}");
    }
}
