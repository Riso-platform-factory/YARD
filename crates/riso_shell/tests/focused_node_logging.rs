#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
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
        captured.spans.insert(
            id,
            format!(
                "{} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ),
        );
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            && event.metadata().target() != "focus_diagnostic"
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

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "focus_diagnostic", "{}", record.args());
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

fn draw(window: gpui::WindowHandle<riso_shell::dev::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn focus_changes_and_blurs_log_a_focus_change_group(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let ext = cx.update(|cx| {
            let ext = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            ext.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            for node in ["a", "b", "c"] {
                ext.place(cx, node, "tour.card", "root", "children");
            }
            ext
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "CARD-b");
        click(&mut visual, "CARD-c");
        // Inside the node that already has the focus: nothing is logged.
        click(&mut visual, "CARD-c");
        cx.update(|cx| ext.set_hidden(cx, "c", true));
        draw(window, cx);
        click(&mut visual, "CARD-b");
        cx.update(|cx| ext.remove(cx, "b"));
        draw(window, cx);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let changes: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .filter(|line| line.contains("focus-change"))
        .collect();
    assert_eq!(
        changes,
        [
            "INFO focus-change node=b: DEBUG message=focused",
            "INFO focus-change node=c: DEBUG message=focused",
            "INFO focus-change node=none: DEBUG message=focused",
            "INFO focus-change node=b: DEBUG message=focused",
            "INFO focus-change node=none: DEBUG message=focused",
        ]
    );
    assert!(
        !captured.lines.iter().any(|line| line.contains(": ERROR")),
        "unexpected error in {:#?}",
        captured.lines
    );
}
