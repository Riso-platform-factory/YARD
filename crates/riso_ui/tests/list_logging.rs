#![cfg(feature = "dev")]

// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.
use gpui::{InteractiveElement, ParentElement, Styled, TestAppContext, div};
use riso_ui::dev::{List, ListHeader, ListItem, ListSeparator};
use riso_ui::{Label, v_flex};
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
        v_flex().size_full().child(
            List::new()
                .header(ListHeader::new("Section"))
                .child(
                    div().debug_selector(|| "PICK".into()).child(
                        ListItem::new("pick")
                            .child(Label::new("Pick"))
                            .on_click(|_, _| {
                                tracing::debug!("handled pick");
                            }),
                    ),
                )
                .child(ListSeparator)
                .child(
                    div().debug_selector(|| "LOCKED".into()).child(
                        ListItem::new("locked")
                            .disabled(true)
                            .child(Label::new("Locked"))
                            .on_click(|_, _| {
                                tracing::debug!("handled locked");
                            }),
                    ),
                ),
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
fn a_draw_logs_nothing_and_each_click_opens_a_group_around_the_owners_line(
    cx: &mut TestAppContext,
) {
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
        assert!(
            !captured
                .lines
                .iter()
                .any(|line| line.contains("riso_ui::dev::list")),
            "drawing a list, a header, items, and a separator logs nothing: {:?}",
            captured.lines
        );
        assert!(
            !captured
                .spans
                .values()
                .any(|span| span.contains("list-item-click")),
            "drawing opens no click group: {:?}",
            captured.spans
        );
        captured.lines.clear();
        captured.entered.clear();
    }

    let pick = cx
        .debug_bounds("PICK")
        .expect("the enabled item's bounds")
        .center();
    let locked = cx
        .debug_bounds("LOCKED")
        .expect("the disabled item's bounds")
        .center();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_click(pick, gpui::Modifiers::default());
        cx.simulate_click(pick, gpui::Modifiers::default());
        cx.simulate_click(locked, gpui::Modifiers::default());
    });

    let captured = trace.0.lock().unwrap();
    let clicks: Vec<_> = captured
        .spans
        .values()
        .filter(|span| span.starts_with("DEBUG list-item-click"))
        .collect();
    assert_eq!(
        clicks.len(),
        2,
        "each click on the enabled item opens one group: {:?}",
        captured.spans
    );
    assert!(
        clicks
            .iter()
            .all(|span| span.contains("id=") && span.contains("pick")),
        "each click group carries the item's id: {clicks:?}"
    );
    let handled: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.starts_with("DEBUG list-item-click") && line.contains("handled pick"))
        .collect();
    assert_eq!(
        handled.len(),
        2,
        "the owner's line is inside each click group: {:?}",
        captured.lines
    );
    assert!(
        !captured
            .lines
            .iter()
            .any(|line| line.contains("handled locked")),
        "a disabled item's handler never runs: {:?}",
        captured.lines
    );
    assert!(
        !captured
            .spans
            .values()
            .any(|span| span.contains("list-item-click") && span.contains("locked")),
        "a disabled item opens no click group: {:?}",
        captured.spans
    );
}
