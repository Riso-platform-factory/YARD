#![cfg(feature = "dev")]

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    records: Vec<String>,
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
}

#[derive(Clone, Default)]
struct TraceCapture(Arc<Mutex<Captured>>);
struct TraceFields(String);
impl tracing::field::Visit for TraceFields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(&mut self.0, " {}={value:?}", field.name()).unwrap();
    }
}
impl tracing::Subscriber for TraceCapture {
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
        captured
            .spans
            .insert(id, attributes.metadata().name().into());
        if attributes.metadata().target().starts_with("riso_ui") {
            captured.records.push(format!(
                "{} {}",
                attributes.metadata().level(),
                attributes.metadata().name()
            ));
        }
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_ui") {
            return;
        }
        let mut fields = TraceFields(event.metadata().level().to_string());
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        assert!(
            captured
                .entered
                .iter()
                .any(|id| captured.spans[id] == "ui-init"),
            "event outside ui-init: {}",
            fields.0
        );
        captured.records.push(fields.0);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

#[gpui::test]
fn init_logs_fonts_and_themes_inside_ui_init(cx: &mut gpui::TestAppContext) {
    let trace = TraceCapture::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(riso_ui::init);
    });
    cx.update(|cx| {
        let rgba = gpui::Rgba::from(riso_ui::palette(cx).background);
        assert_eq!(
            (
                (rgba.r * 255.).round() as u8,
                (rgba.g * 255.).round() as u8,
                (rgba.b * 255.).round() as u8
            ),
            (0x3b, 0x41, 0x4d)
        );
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    // The test text system knows no font names, so whether the font was found is seen in the running application, not here.
    assert!(captured.records[1].starts_with("DEBUG message=bundled fonts loaded ibm_plex_sans="));
    assert_eq!(
        captured.records,
        [
            "INFO ui-init",
            captured.records[1].as_str(),
            "DEBUG message=bundled themes loaded themes=2",
            "DEBUG message=theme active theme=One Dark",
        ]
    );
}
