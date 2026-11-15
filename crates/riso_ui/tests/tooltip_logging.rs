#![cfg(feature = "dev")]

// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.
use gpui::{
    InteractiveElement, KeyBinding, Modifiers, ParentElement, Styled, TestAppContext,
    VisualTestContext, div, px, size,
};
use riso_ui::{Button, dev::Tooltip, v_flex};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

gpui::actions!(tooltip_logging_test, [Quit]);

#[cfg(target_os = "macos")]
const QUIT_KEYS: &str = "cmd-q";
#[cfg(not(target_os = "macos"))]
const QUIT_KEYS: &str = "ctrl-q";

// The keys' text as Zed's `text_for_keystroke` writes it on this OS.
#[cfg(target_os = "macos")]
const QUIT_TEXT: &str = "Command-Q";
#[cfg(not(target_os = "macos"))]
const QUIT_TEXT: &str = "Ctrl-Q";

struct Page;

impl gpui::Render for Page {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex().size_full().child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .debug_selector(|| "QUIT".into())
                .child(
                    Button::new("quit", "Quit")
                        .tooltip(Tooltip::for_action("Quit the gallery", &Quit)),
                ),
        )
    }
}

// A window of a known size: 400px wide, 600px tall.
fn open_test_window(
    view: impl gpui::Render + 'static,
    cx: &mut TestAppContext,
) -> VisualTestContext {
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| view);
    VisualTestContext::from_window(window.into(), cx)
}

// Draws a frame so `debug_bounds` sees the painted elements.
fn draw_a_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
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
        write!(&mut self.0, " {}={value}", field.name()).unwrap();
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(&mut self.0, " {}={value:?}", field.name()).unwrap();
    }
}

// Captures only `riso_ui` items at DEBUG or above, so an empty capture is
// exactly "nothing at DEBUG or above from riso_ui".
impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_ui") && *metadata.level() <= tracing::Level::DEBUG
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
fn resting_the_pointer_logs_nothing_and_the_shown_tooltip_logs_one_group_with_its_fields(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new(QUIT_KEYS, Quit, None)]);
    });
    let mut cx = open_test_window(Page, cx);
    draw_a_frame(&mut cx);
    let rest = cx.debug_bounds("QUIT").expect("the button draws").center();
    let trace = Trace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_mouse_move(rest, None, Modifiers::default());
    });
    {
        let captured = trace.0.lock().unwrap();
        assert!(
            captured.spans.is_empty() && captured.lines.is_empty(),
            "a move over a tooltip'd element logs nothing at DEBUG or above: {:?} {:?}",
            captured.spans,
            captured.lines
        );
    }

    tracing::subscriber::with_default(trace.clone(), || {
        cx.executor().advance_clock(Duration::from_millis(600));
        cx.run_until_parked();
        draw_a_frame(&mut cx);
    });
    let captured = trace.0.lock().unwrap();
    let groups: Vec<_> = captured
        .spans
        .values()
        .filter(|span| span.starts_with("INFO tooltip-show"))
        .collect();
    assert_eq!(
        groups.len(),
        1,
        "showing the tooltip opens one `tooltip-show` group: {:?}",
        captured.spans
    );
    assert!(
        groups[0].contains("title=") && groups[0].contains("Quit the gallery"),
        "the group carries the title: {}",
        groups[0]
    );
    let built: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tooltip built"))
        .collect();
    assert_eq!(
        built.len(),
        1,
        "one `tooltip built` line: {:?}",
        captured.lines
    );
    assert!(
        built[0].starts_with("INFO tooltip-show"),
        "the built line sits inside the group: {}",
        built[0]
    );
    assert!(
        built[0].contains("DEBUG riso_ui")
            && built[0].contains("keys=")
            && built[0].contains(QUIT_TEXT),
        "the line is a riso_ui DEBUG naming the bound keys: {}",
        built[0]
    );
    assert!(
        built[0].contains("meta=false"),
        "the line says the tooltip has no detail line: {}",
        built[0]
    );
}
