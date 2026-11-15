#![cfg(feature = "dev")]

use gpui::{AppContext, TestAppContext};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    stack: Vec<u64>,
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
    fn new_span(&self, attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut capture = self.0.lock().unwrap();
        let id = capture.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        capture
            .spans
            .insert(id, format!("{}{}", attrs.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut capture = self.0.lock().unwrap();
        let group = capture
            .stack
            .last()
            .map(|id| capture.spans[id].clone())
            .unwrap_or_default();
        capture
            .lines
            .push(format!("{group}: {}{}", event.metadata().level(), fields.0));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().stack.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().stack.pop(), Some(id.into_u64()));
    }
}
struct Model;

#[gpui::test]
fn contexts_capture_declaration_creation_lookup_and_rule_groups(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let ext = cx.update(|cx| {
            riso_ui::init(cx);
            let ext = shell::Extension::new(cx, "edge").unwrap();
            ext.provide::<Model>(cx, "edge.giver", |_, cx| cx.new(|_| Model));
            ext.require_context::<Model>(cx, "edge.leaf");
            for component in ["edge.giver", "edge.leaf"] {
                ext.register(cx, component, &["body"], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
            }
            ext.place_root(cx, "root", "riso.row");
            ext.place(cx, "giver", "edge.giver", "root", "children");
            ext.place(cx, "leaf", "edge.leaf", "giver", "body");
            ext.place(cx, "stray", "edge.leaf", "root", "children");
            ext
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        window
            .update(cx, |_, window, cx| {
                ext.add(window, cx, "added", "edge.leaf", "root", "children");
                assert_eq!(shell::user_layer_len(cx), 0);
                assert!(shell::context::<Model>(cx, "added").is_none());
            })
            .unwrap();
        cx.update(|cx| {
            assert!(shell::context::<Model>(cx, "leaf").is_some());
            ext.move_node(cx, "leaf", "root", "children", 0);
            assert_eq!(shell::user_layer_len(cx), 0);
            assert!(shell::context::<Model>(cx, "stray").is_none());
            assert!(shell::context::<Model>(cx, "root").is_none());
            let model = cx.new(|_| Model);
            ext.provide_window(cx, model.clone());
            assert_eq!(
                shell::context::<Model>(cx, "root").unwrap().entity_id(),
                model.entity_id()
            );
        });
    });
    let capture = trace.0.lock().unwrap();
    assert!(capture.stack.is_empty());
    for kind in ["provide", "require", "window"] {
        assert!(
            capture
                .lines
                .iter()
                .any(|line| line.starts_with("context-declare")
                    && line.contains(kind)
                    && line.contains("Model")
                    && line.contains("DEBUG message=declared")),
            "{:?}",
            capture.lines
        );
    }
    assert!(
        capture
            .lines
            .iter()
            .any(|line| line.starts_with("component-create node=giver")
                && line.contains("DEBUG")
                && line.contains("context created")
                && line.contains("model=\"Model\"")),
        "{:?}",
        capture.lines
    );
    for from in ["giver", "none", "window"] {
        assert!(
            capture.lines.iter().any(|line| line.contains("TRACE")
                && line.contains("message=context")
                && line.contains(&format!("from=\"{from}\""))),
            "{:?}",
            capture.lines
        );
    }
    for (group, message) in [
        (
            "node-place node=\"stray\" component=\"edge.leaf\" parent=\"root\" slot=\"children\": INFO",
            "stray requires a Model model, and none is above root",
        ),
        (
            "layout-op op=\"add\" node=\"added\" extension=\"edge\": INFO",
            "added requires a Model model, and none is above root",
        ),
    ] {
        assert!(
            capture
                .lines
                .iter()
                .any(|line| line.starts_with(group) && line.contains(message)),
            "{:?}",
            capture.lines
        );
    }
    assert!(
        capture.lines.iter().any(|line| line
            .starts_with("layout-op op=\"move\" node=\"leaf\" extension=\"edge\": INFO")
            && line.contains("leaf requires a Model model, and none is above root")),
        "{:?}",
        capture.lines
    );
}
