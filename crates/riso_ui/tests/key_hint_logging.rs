// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.
use gpui::{InteractiveElement, KeyBinding, ParentElement, Styled};
use riso_ui::{KeyHint, v_flex};
use std::sync::{Arc, Mutex};

gpui::actions!(key_hint_test, [ToggleZoom]);

struct Page;

impl gpui::Render for Page {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .key_context("Shell")
            .child(KeyHint::for_action(&ToggleZoom, cx))
    }
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

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
        let id = tracing::span::Id::from_u64(captured.len() as u64 + 1);
        captured.push(format!(
            "group {} {}",
            attributes.metadata().level(),
            attributes.metadata().name()
        ));
        id
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(format!("line {}", event.metadata().level()));
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[gpui::test]
fn drawing_a_hint_logs_no_group_and_no_line_at_debug_or_above(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new("shift-escape", ToggleZoom, Some("Shell"))]);
    });
    let window = cx.add_window(|_, _| Page);
    let trace = Trace::default();

    // Two draws: the second resolves the binding against the first frame's
    // keymap state, so the resolution path itself is exercised.
    tracing::subscriber::with_default(trace.clone(), || {
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
                let _ = window.draw(cx);
            })
            .unwrap();
    });

    let captured = trace.0.lock().unwrap();
    assert!(
        captured.is_empty(),
        "a hint reads the keymap and changes nothing: {captured:?}"
    );
}
