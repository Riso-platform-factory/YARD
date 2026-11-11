use gpui::{AppContext, TestAppContext};
use std::cell::RefCell;
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    lines: Vec<String>,
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    creation_fields: BTreeMap<u64, String>,
    parents: Vec<(String, Option<String>)>,
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
        captured
            .spans
            .insert(id, attributes.metadata().name().into());
        if attributes.metadata().target().starts_with("riso_shell") {
            let mut fields = Fields::default();
            attributes.record(&mut fields);
            if attributes.metadata().name() == "component-create" {
                let parent = captured
                    .entered
                    .iter()
                    .rev()
                    .find_map(|id| captured.creation_fields.get(id).cloned());
                captured.parents.push((fields.0.clone(), parent));
                captured.creation_fields.insert(id, fields.0.clone());
            }
            captured.lines.push(format!(
                "{} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ));
        }
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::TRACE {
            return;
        }
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && target != "component_diagnostic"
        {
            return;
        }
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].clone())
            .expect("operation event inside a group");
        let mut fields = Fields::default();
        event.record(&mut fields);
        captured.lines.push(format!(
            "  {group}: {}{}",
            event.metadata().level(),
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

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "component_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

struct Probe {
    bounds: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    renders: Rc<Cell<usize>>,
}
impl gpui::Render for Probe {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        self.renders.set(self.renders.get() + 1);
        let bounds = self.bounds.clone();
        // No size on the view: the shell must stretch it in both axes.
        gpui::canvas(
            move |measured, _, _| bounds.set(Some(measured)),
            |_, _, _, _| {},
        )
    }
}

fn open(cx: &TestAppContext) -> gpui::WindowHandle<riso_shell::Shell> {
    cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    })
}

#[gpui::test]
fn component_operations_log_in_order_and_create_once(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);
    let trace = Trace::default();
    let creates = Rc::new(Cell::new(0));
    let bounds = Rc::new(Cell::new(None));
    let renders = Rc::new(Cell::new(0));
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let creates = creates.clone();
            let bounds = bounds.clone();
            let renders = renders.clone();
            riso_shell::register(cx, "tour.card", &[], move |node, _, _, cx| {
                assert_eq!(node.as_str(), "root");
                creates.set(creates.get() + 1);
                // The global borrow must be released before invoking the factory.
                riso_shell::register(cx, "tour.extra", &[], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
                cx.new(|_| Probe {
                    bounds: bounds.clone(),
                    renders: renders.clone(),
                })
                .into()
            });
            riso_shell::register(cx, "tour.card", &[], |_, _, _, _| {
                panic!("duplicate replaced first factory")
            });
            riso_shell::register(cx, "Bad.Card", &[], |_, _, _, _| {
                panic!("invalid factory was registered")
            });
            riso_shell::place_root(cx, "0root", "tour.card");
            riso_shell::place_root(cx, "root", "Bad.Card");
            riso_shell::place_root(cx, "root", "tour.card");
            riso_shell::place_root(cx, "second", "tour.card");
        });
        let window = open(cx);
        for _ in 0..2 {
            gpui::AnyWindowHandle::from(window)
                .update(cx, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                    assert_eq!(bounds.get().unwrap().size, window.viewport_size());
                })
                .unwrap();
        }
        assert!(renders.get() >= 2);
        assert_eq!(creates.get(), 1);
        let empty = cx.new_app();
        empty.update(riso_ui::init);
        let _window = open(&empty);
        let mut missing = cx.new_app();
        missing.update(riso_ui::init);
        missing.update(|cx| riso_shell::place_root(cx, "missing", "tour.missing"));
        let window = open(&missing);
        for _ in 0..2 {
            gpui::AnyWindowHandle::from(window)
                .update(&mut missing, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
        }
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines = &captured.lines;
    assert_eq!(
        lines,
        &[
            r#"INFO component-register component="tour.card" slots="""#,
            "  component-register: DEBUG message=registered",
            r#"INFO component-register component="tour.card" slots="""#,
            "  component-register: ERROR message=component tour.card is already registered",
            r#"INFO component-register component="Bad.Card" slots="""#,
            r#"  component-register: ERROR message=invalid component ID "Bad.Card": a slug must start with a lowercase ASCII letter"#,
            r#"INFO root-place node="0root" component="tour.card""#,
            r#"  root-place: ERROR message=invalid node ID "0root": a slug must start with a lowercase ASCII letter"#,
            r#"INFO root-place node="root" component="Bad.Card""#,
            r#"  root-place: ERROR message=invalid component ID "Bad.Card": a slug must start with a lowercase ASCII letter"#,
            r#"INFO root-place node="root" component="tour.card""#,
            "  root-place: DEBUG message=root placed node=root component=tour.card",
            r#"INFO root-place node="second" component="tour.card""#,
            "  root-place: INFO message=a root is already placed node=second component=tour.card root=root",
            "INFO shell-create root=root",
            "INFO component-create node=root component=tour.card",
            r#"INFO component-register component="tour.extra" slots="""#,
            "  component-register: DEBUG message=registered",
            "INFO shell-create root=none",
            "  shell-create: INFO message=no root placed; the window shows the background",
            r#"INFO root-place node="missing" component="tour.missing""#,
            "  root-place: DEBUG message=root placed node=missing component=tour.missing",
            "INFO shell-create root=missing",
            "INFO component-create node=missing component=tour.missing",
            "  component-create: ERROR message=node missing names unregistered component tour.missing",
        ]
    );
    drop(captured);
    slots_contract(cx);
}

fn slots_contract(cx: &mut TestAppContext) {
    let trace = Trace::default();
    let handles = Rc::new(RefCell::new(BTreeMap::new()));
    let created = Rc::new(RefCell::new(Vec::new()));
    let weak_views = Rc::new(RefCell::new(Vec::new()));
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(riso_ui::init);
        app.update(|cx| {
            let handles = handles.clone();
            let created = created.clone();
            let weak_views = weak_views.clone();
            riso_shell::register(
                cx,
                "tour.card",
                &["body", "other"],
                move |node, slots, _, cx| {
                    // Parent-first creation: no child has been created yet.
                    assert!(slots.views("body").is_empty());
                    assert!(slots.views("unknown").is_empty());
                    handles.borrow_mut().insert(node.to_string(), slots);
                    created.borrow_mut().push(node.to_string());
                    let view: gpui::AnyView = cx.new(|_| gpui::Empty).into();
                    weak_views.borrow_mut().push(view.downgrade());
                    view
                },
            );
            for (component, slots) in [
                ("tour.badslot", vec!["Bad"]),
                ("tour.double", vec!["body", "body"]),
            ] {
                riso_shell::register(cx, component, &slots, |_, _, _, _| {
                    panic!("refused registration used")
                });
                // Refusal must leave the ID free for a valid registration.
                riso_shell::register(cx, component, &[], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
            }
            riso_shell::place_root(cx, "root", "tour.card");
            for (node, component, parent, slot) in [
                ("Bad", "tour.card", "root", "body"),
                ("bad-component", "Bad.Card", "root", "body"),
                ("bad-parent", "tour.card", "Bad", "body"),
                ("bad-slot", "tour.card", "root", "Bad"),
                ("lost", "tour.card", "nowhere", "body"),
                ("root", "tour.card", "root", "body"),
                ("left", "tour.card", "root", "body"),
                ("inner", "tour.card", "left", "body"),
                ("left", "tour.card", "root", "other"),
                ("missing", "tour.missing", "root", "body"),
                ("hidden", "tour.card", "missing", "body"),
                ("aside", "tour.card", "root", "side"),
                ("hidden-aside", "tour.card", "aside", "body"),
                ("right", "tour.card", "root", "body"),
                ("last", "tour.card", "root", "other"),
            ] {
                riso_shell::place(cx, node, component, parent, slot);
            }
        });
        let window = open(&app);
        assert_eq!(
            *created.borrow(),
            ["root", "left", "inner", "right", "last"]
        );
        {
            let handles = handles.borrow();
            let root = &handles["root"];
            let root_body = root.views("body");
            assert_eq!(root_body.len(), 2);
            let left_body = handles["left"].views("body");
            let other = root.views("other");
            let weak = weak_views.borrow();
            assert_eq!(
                root_body[0].entity_id(),
                weak[1].upgrade().unwrap().entity_id()
            );
            assert_eq!(
                root_body[1].entity_id(),
                weak[3].upgrade().unwrap().entity_id()
            );
            assert_eq!(
                left_body[0].entity_id(),
                weak[2].upgrade().unwrap().entity_id()
            );
            assert_eq!(other[0].entity_id(), weak[4].upgrade().unwrap().entity_id());
            assert!(root.views("side").is_empty());
            assert!(root.views("unknown").is_empty());
            assert!(handles["right"].views("body").is_empty());
            drop(root_body);
            drop(left_body);
            drop(other);
        }
        handles.borrow_mut().clear();
        let weak = weak_views.borrow();
        // The views are retained even though Empty never renders or keeps the slots.
        for _ in 0..2 {
            gpui::AnyWindowHandle::from(window)
                .update(&mut app, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
        }
        assert!(weak.iter().all(|view| view.upgrade().is_some()));
        assert_eq!(created.borrow().len(), 5);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines = &captured.lines;
    for fragment in [
        "component-register component=\"tour.card\" slots=\"body,other\"",
        "component-register: ERROR message=invalid slot name \"Bad\"",
        "component-register: ERROR message=component tour.double declares slot body twice",
        "node-place: ERROR message=invalid node ID \"Bad\"",
        "node-place: ERROR message=invalid component ID \"Bad.Card\"",
        "node-place: ERROR message=invalid parent ID \"Bad\"",
        "node-place: ERROR message=invalid slot name \"Bad\"",
        "node-place: INFO message=the parent is not in the tree node=lost",
        "node-place: INFO message=the node ID is already in the tree node=root",
        "node-place: INFO message=the node ID is already in the tree node=left",
        "node-place: DEBUG message=node placed node=left component=tour.card parent=root slot=body",
        "component-create: ERROR message=node missing names unregistered component tour.missing",
        "component-create: ERROR message=node aside is placed in slot side not declared by parent component tour.card",
    ] {
        assert!(
            lines.iter().any(|line| line.contains(fragment)),
            "missing {fragment:?} in {lines:#?}"
        );
    }
    assert_eq!(
        lines.iter().filter(|line| line.contains(": ERROR")).count(),
        8
    );
    let root = " node=root component=tour.card";
    let left = " node=left component=tour.card";
    assert_eq!(
        captured.parents,
        [
            (root.into(), None),
            (left.into(), Some(root.into())),
            (" node=inner component=tour.card".into(), Some(left.into())),
            (
                " node=missing component=tour.missing".into(),
                Some(root.into())
            ),
            (" node=right component=tour.card".into(), Some(root.into())),
            (" node=aside component=tour.card".into(), Some(root.into())),
            (" node=last component=tour.card".into(), Some(root.into())),
        ]
    );
}
