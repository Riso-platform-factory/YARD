use gpui::{AppContext, TestAppContext};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};
use yard_shell as shell;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

impl tracing::Subscriber for Trace {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

impl Trace {
    fn refuses(&self, expected: &str, run: impl FnOnce()) {
        self.0.lock().unwrap().clear();
        tracing::subscriber::with_default(self.clone(), run);
        let lines = self.0.lock().unwrap();
        assert!(
            lines.iter().any(|line| line.contains(expected)),
            "{expected}: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("requires a Count")),
            "{lines:?}"
        );
    }
}

struct Count;
struct Other;

#[gpui::test]
fn requirements_use_ancestors_and_models_follow_view_lifetimes(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let creates = Rc::new(Cell::new(0));
    let ext = cx.update(|cx| {
        yard_ui::init(cx);
        let ext = shell::Extension::new(cx, "edge").unwrap();
        for component in ["edge.giver", "edge.both", "edge.leaf"] {
            ext.register(cx, component, &["body"], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
        }
        let calls = creates.clone();
        ext.provide::<Count>(cx, "edge.giver", move |_, cx| {
            calls.set(calls.get() + 1);
            cx.new(|_| Count)
        });
        ext.provide::<Other>(cx, "edge.giver", |_, cx| cx.new(|_| Other));
        ext.require_context::<Count>(cx, "edge.both");
        ext.provide::<Count>(cx, "edge.both", |_, cx| cx.new(|_| Count));
        ext.place_root(cx, "root", "yard.row");
        // Its own provider cannot satisfy its requirement.
        ext.place(cx, "refused", "edge.both", "root", "children");
        ext.place(cx, "giver", "edge.giver", "root", "children");
        ext.place(cx, "both", "edge.both", "giver", "body");
        ext.place(cx, "leaf", "edge.leaf", "both", "body");
        ext.place(cx, "outside", "edge.leaf", "root", "children");
        ext
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    assert_eq!(creates.get(), 1);
    let (ancestor, inner, weak) = cx.update(|cx| {
        assert!(shell::context::<Count>(cx, "refused").is_none());
        let ancestor = shell::context::<Count>(cx, "both").unwrap();
        let inner = shell::context::<Count>(cx, "leaf").unwrap();
        assert!(shell::context::<Other>(cx, "leaf").is_some());
        assert_ne!(ancestor.entity_id(), inner.entity_id());
        (
            ancestor.entity_id(),
            inner.entity_id(),
            ancestor.downgrade(),
        )
    });
    cx.update(|cx| {
        ext.set_hidden(cx, "giver", true);
        ext.set_hidden(cx, "giver", false);
        ext.move_node(cx, "giver", "outside", "body", 0);
        assert_eq!(
            shell::context::<Count>(cx, "both").unwrap().entity_id(),
            ancestor
        );
        assert_eq!(
            shell::context::<Count>(cx, "leaf").unwrap().entity_id(),
            inner
        );
        // Moving both out would leave its own requirement unsatisfied.
        let before = shell::user_layer_len(cx);
        ext.move_node(cx, "both", "root", "children", 0);
        assert_eq!(shell::user_layer_len(cx), before);
    });
    window
        .update(cx, |_, window, cx| ext.reset_layout(window, cx))
        .unwrap();
    assert_eq!(creates.get(), 1);
    cx.update(|cx| ext.remove(cx, "giver"));
    cx.update(|_| {});
    assert!(weak.upgrade().is_none());
    window
        .update(cx, |_, window, cx| ext.reset_layout(window, cx))
        .unwrap();
    assert_eq!(creates.get(), 2);
    cx.update(|cx| {
        assert_ne!(
            shell::context::<Count>(cx, "both").unwrap().entity_id(),
            ancestor
        )
    });
}

#[gpui::test]
fn restore_skips_a_move_without_context_and_applies_the_next_operation(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    struct Saved(std::path::PathBuf);
    impl Drop for Saved {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Saved(std::env::temp_dir().join(format!(
        "yard-shell-contexts-edges-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos()),
    )));
    std::fs::create_dir(&directory.0).unwrap();
    let file = directory.0.join("layout.json");
    std::fs::write(&file, r#"{"version":1,"operations":[{"op":"move","node":"leaf","parent":"root","slot":"children"},{"op":"hide","node":"sibling"}]}"#).unwrap();
    cx.update(|cx| {
        yard_ui::init(cx);
        shell::set_layout_file(cx, file.clone());
        let ext = shell::Extension::new(cx, "edge").unwrap();
        ext.provide::<Count>(cx, "edge.giver", |_, cx| cx.new(|_| Count));
        ext.require_context::<Count>(cx, "edge.leaf");
        for component in ["edge.giver", "edge.leaf"] {
            ext.register(cx, component, &["body"], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
        }
        ext.place_root(cx, "root", "yard.row");
        ext.place(cx, "giver", "edge.giver", "root", "children");
        ext.place(cx, "leaf", "edge.leaf", "giver", "body");
        ext.place(cx, "sibling", "edge.leaf", "giver", "body");
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap();
        assert_eq!(shell::user_layer_len(cx), 1);
        assert!(shell::is_hidden(cx, "sibling"));
        let leaf = shell::context::<Count>(cx, "leaf").unwrap();
        let sibling = shell::context::<Count>(cx, "sibling").unwrap();
        assert_eq!(leaf.entity_id(), sibling.entity_id());
    });
}

#[gpui::test]
fn shell_refusals_precede_context_and_new_nodes_have_no_subtree(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    let ext = cx.update(|cx| {
        yard_ui::init(cx);
        let ext = shell::Extension::new(cx, "edge").unwrap();
        for component in ["edge.giver", "edge.leaf", "edge.plain"] {
            ext.register(cx, component, &["body"], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
        }
        ext.provide::<Count>(cx, "edge.giver", |_, cx| cx.new(|_| Count));
        ext.require_context::<Count>(cx, "edge.leaf");
        ext.require_context::<Count>(cx, "edge.unregistered");
        ext.place_root(cx, "root", "yard.row");
        ext.place(cx, "giver", "edge.giver", "root", "children");
        ext.place(cx, "branch", "edge.plain", "giver", "body");
        ext.place(cx, "leaf", "edge.leaf", "branch", "body");
        for component in ["edge.plain", "edge.leaf"] {
            trace.refuses("the node ID is already in the tree", || {
                ext.place(cx, "branch", component, "root", "children")
            });
        }
        ext
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    window
        .update(cx, |_, window, cx| {
            for component in ["edge.plain", "edge.leaf"] {
                trace.refuses("the node ID is already in the tree", || {
                    ext.add(window, cx, "branch", component, "root", "children")
                });
            }
            trace.refuses("component is not registered", || {
                ext.add(window, cx, "new", "edge.unregistered", "root", "children")
            });
            trace.refuses("parent component does not declare slot", || {
                ext.add(window, cx, "new", "edge.leaf", "root", "missing")
            });
            trace.refuses("parent component does not declare slot", || {
                ext.move_node(cx, "leaf", "root", "missing", 0)
            });
            assert_eq!(shell::user_layer_len(cx), 0);
        })
        .unwrap();
}
