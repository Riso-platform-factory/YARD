#![cfg(feature = "dev")]

// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.
use gpui::{
    InteractiveElement, IntoElement, ParentElement, ScrollDelta, ScrollEvent, Styled,
    TestAppContext, UniformListScrollHandle, div, point, px,
};
use riso_ui::dev::ScrollList;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex},
};

struct Rows {
    handle: UniformListScrollHandle,
    ranges: Rc<RefCell<Vec<Range<usize>>>>,
}

impl gpui::Render for Rows {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let ranges = self.ranges.clone();
        div().size_full().child(
            div().debug_selector(|| "LIST".into()).w(px(300.)).child(
                ScrollList::new(
                    "scroll-rows",
                    1000,
                    &self.handle,
                    move |range: Range<usize>, _: &mut gpui::Window, _: &mut gpui::App| {
                        ranges.borrow_mut().push(range.clone());
                        range
                            .map(|ix| {
                                div()
                                    .h(px(20.))
                                    .child(format!("Row {ix}"))
                                    .into_any_element()
                            })
                            .collect()
                    },
                )
                .h(px(200.))
                .w(px(300.)),
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
fn a_list_logs_rows_drawn_once_per_frame_at_trace_with_the_ranges_fields(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let ranges = Rc::new(RefCell::new(Vec::new()));
    let (_view, cx) = cx.add_window_view(|_, _| Rows {
        handle: UniformListScrollHandle::new(),
        ranges: ranges.clone(),
    });
    let trace = Trace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    });

    {
        let mut captured = trace.0.lock().unwrap();
        let drawn: Vec<&String> = captured
            .lines
            .iter()
            .filter(|line| line.contains("rows drawn"))
            .collect();
        assert_eq!(
            drawn.len(),
            1,
            "one `rows drawn` line per drawn frame: {:?}",
            captured.lines
        );
        let line = drawn[0];
        assert!(line.contains("TRACE"), "`rows drawn` is at trace: {line}");
        assert!(
            line.contains("id=") && line.contains("scroll-rows"),
            "`rows drawn` carries the list's id: {line}"
        );
        let visible = ranges
            .borrow()
            .last()
            .cloned()
            .expect("the frame asked for rows");
        assert!(
            line.contains(&format!("first={}", visible.start)),
            "`rows drawn` carries the range's first row: {line} vs {visible:?}"
        );
        assert!(
            line.contains(&format!("count={}", visible.len())),
            "`rows drawn` carries the range's row count: {line} vs {visible:?}"
        );
        assert!(
            !captured
                .lines
                .iter()
                .any(|line| line.contains("riso_ui") && !line.contains("rows drawn")),
            "drawing a list logs nothing else: {:?}",
            captured.lines
        );
        captured.lines.clear();
        captured.entered.clear();
    }

    // A scroll changes the range the next frame's line carries.
    let list = cx.debug_bounds("LIST").expect("the list draws");
    cx.simulate_event(ScrollEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-200.))),
        ..Default::default()
    });
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    });

    let captured = trace.0.lock().unwrap();
    let drawn: Vec<&String> = captured
        .lines
        .iter()
        .filter(|line| line.contains("rows drawn"))
        .collect();
    assert_eq!(
        drawn.len(),
        1,
        "the scrolled frame logs one `rows drawn` line of its own: {:?}",
        captured.lines
    );
    let line = drawn[0];
    let visible = ranges
        .borrow()
        .last()
        .cloned()
        .expect("the frame asked for rows");
    assert!(visible.start > 0, "the list scrolled: {visible:?}");
    assert!(
        line.contains(&format!("first={}", visible.start))
            && line.contains(&format!("count={}", visible.len())),
        "`rows drawn` follows the scrolling: {line} vs {visible:?}"
    );
}
