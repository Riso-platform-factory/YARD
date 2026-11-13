// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.
use gpui::{InteractiveElement, ParentElement, Styled, TestAppContext, div};
use riso_ui::{Button, IconButton, IconName, v_flex};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

struct Page;

impl gpui::Render for Page {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "GREETING".into())
                    .child(Button::new("greeting", "Hello").on_click(|_, _| {
                        tracing::debug!("handled greeting");
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "QUIET".into())
                    .child(
                        Button::new("quiet", "Nope")
                            .disabled(true)
                            .on_click(|_, _| {
                                tracing::debug!("handled quiet");
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .debug_selector(|| "ICON".into())
                    .child(
                        IconButton::new("icon-button", IconName::Check).on_click(|_, _| {
                            tracing::debug!("handled icon");
                        }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Button::new("toggle-label", "On").toggle_state(true)),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(IconButton::new("toggle-icon", IconName::Check).toggle_state(true)),
            )
    }
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
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        write!(self.0, " {}={value}", field.name()).unwrap();
    }

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
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .and_then(|id| captured.spans.get(id))
            .map_or_else(|| "none".to_string(), String::clone);
        captured.lines.push(format!(
            "{group}: {} {}{}",
            event.metadata().level(),
            event.metadata().target(),
            fields.0
        ));
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

#[gpui::test]
fn a_draw_logs_each_buttons_id_and_a_click_groups_the_owners_line(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_page, cx) = cx.add_window_view(|_, _| Page);
    let trace = Trace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    });

    {
        let mut captured = trace.0.lock().unwrap();
        for id in [
            "greeting",
            "quiet",
            "icon-button",
            "toggle-label",
            "toggle-icon",
        ] {
            let draws: Vec<_> = captured
                .lines
                .iter()
                .filter(|line| line.starts_with("none: TRACE riso_ui") && line.contains(id))
                .collect();
            assert_eq!(
                draws.len(),
                1,
                "one trace line must name {id}: {:?}",
                captured.lines
            );
        }
        let greeting = captured
            .lines
            .iter()
            .find(|line| line.contains("greeting"))
            .unwrap();
        assert!(
            greeting.contains("Subtle") && greeting.contains("Default"),
            "the draw line names the style and size: {greeting}"
        );
        assert!(
            greeting.contains("disabled=false"),
            "the draw line says the button is enabled: {greeting}"
        );
        let quiet = captured
            .lines
            .iter()
            .find(|line| line.contains("quiet"))
            .unwrap();
        assert!(
            quiet.contains("disabled=true"),
            "the draw line says the button is disabled: {quiet}"
        );
        for id in ["toggle-label", "toggle-icon"] {
            let line = captured
                .lines
                .iter()
                .find(|line| line.contains(id))
                .unwrap();
            assert!(
                line.contains("toggled=true")
                    && line.contains("style=Subtle")
                    && line.contains("size=Default"),
                "the draw line says the button is toggled, with its style and size: {line}"
            );
        }
        captured.lines.clear();
        captured.entered.clear();
    }

    let greeting = cx
        .debug_bounds("GREETING")
        .expect("greeting bounds")
        .center();
    let quiet = cx.debug_bounds("QUIET").expect("quiet bounds").center();
    let icon = cx.debug_bounds("ICON").expect("icon bounds").center();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_click(greeting, gpui::Modifiers::default());
        cx.simulate_click(quiet, gpui::Modifiers::default());
        cx.simulate_click(icon, gpui::Modifiers::default());
    });

    let captured = trace.0.lock().unwrap();
    let clicks: Vec<_> = captured
        .spans
        .values()
        .filter(|span| span.starts_with("DEBUG button-click"))
        .collect();
    assert_eq!(
        clicks.len(),
        2,
        "the two enabled clicks each open a group: {:?}",
        captured.spans
    );
    assert!(
        clicks
            .iter()
            .any(|span| span.contains("id=") && span.contains("greeting")),
        "the click group carries the button's id: {clicks:?}"
    );
    assert!(
        clicks
            .iter()
            .any(|span| span.contains("id=") && span.contains("icon-button")),
        "the click group carries the button's id: {clicks:?}"
    );
    assert!(
        captured
            .lines
            .iter()
            .any(|line| line.starts_with("DEBUG button-click")
                && line.contains("handled greeting")),
        "the owner's line is inside the click group: {:?}",
        captured.lines
    );
    assert!(
        captured
            .lines
            .iter()
            .any(|line| line.starts_with("DEBUG button-click") && line.contains("handled icon")),
        "the owner's line is inside the click group: {:?}",
        captured.lines
    );
    assert!(
        !captured
            .lines
            .iter()
            .any(|line| line.contains("handled quiet")),
        "a disabled button is never clicked: {:?}",
        captured.lines
    );
}
