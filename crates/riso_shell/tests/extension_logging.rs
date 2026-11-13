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
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            let factory_ext = ext.clone();
            let creates = creates.clone();
            let bounds = bounds.clone();
            let renders = renders.clone();
            ext.register(cx, "tour.card", &[], move |node, _, _, cx| {
                assert_eq!(node.as_str(), "root");
                creates.set(creates.get() + 1);
                // The global borrow must be released before invoking the factory.
                factory_ext.register(cx, "tour.extra", &[], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
                cx.new(|_| Probe {
                    bounds: bounds.clone(),
                    renders: renders.clone(),
                })
                .into()
            });
            ext.register(cx, "tour.card", &[], |_, _, _, _| {
                panic!("duplicate replaced first factory")
            });
            ext.register(cx, "Bad.Card", &[], |_, _, _, _| {
                panic!("invalid factory was registered")
            });
            ext.place_root(cx, "0root", "tour.card");
            ext.place_root(cx, "root", "Bad.Card");
            ext.place_root(cx, "root", "tour.card");
            ext.place_root(cx, "second", "tour.card");
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
        missing.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            ext.place_root(cx, "missing", "tour.missing");
        });
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
            concat!(
                "  component-register: ERROR message=invalid component ID ",
                "\"Bad.Card\": a slug must start with a lowercase ASCII letter",
            ),
            r#"INFO root-place node="0root" component="tour.card""#,
            concat!(
                "  root-place: ERROR message=invalid node ID \"0root\": a slug must ",
                "start with a lowercase ASCII letter",
            ),
            r#"INFO root-place node="root" component="Bad.Card""#,
            concat!(
                "  root-place: ERROR message=invalid component ID \"Bad.Card\": a ",
                "slug must start with a lowercase ASCII letter",
            ),
            r#"INFO root-place node="root" component="tour.card""#,
            "  root-place: DEBUG message=root placed node=root component=tour.card extension=tour",
            r#"INFO root-place node="second" component="tour.card""#,
            concat!(
                "  root-place: INFO message=a root is already placed node=second ",
                "component=tour.card extension=tour root=root",
            ),
            "INFO shell-create root=root",
            "INFO component-create node=root component=tour.card extension=tour",
            r#"INFO component-register component="tour.extra" slots="""#,
            "  component-register: DEBUG message=registered",
            "INFO shell-create root=none",
            "  shell-create: INFO message=no root placed; the window shows the background",
            r#"INFO root-place node="missing" component="tour.missing""#,
            "  root-place: DEBUG message=root placed node=missing component=tour.missing extension=tour",
            "INFO shell-create root=missing",
            "INFO component-create node=missing component=tour.missing extension=tour",
            concat!(
                "  component-create: ERROR message=node missing names ",
                "unregistered component tour.missing",
            ),
        ]
    );
    drop(captured);
    slots_contract(cx);
    extensions_contract(cx);
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
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            let handles = handles.clone();
            let created = created.clone();
            let weak_views = weak_views.clone();
            ext.register(
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
                ext.register(cx, component, &slots, |_, _, _, _| {
                    panic!("refused registration used")
                });
                // Refusal must leave the ID free for a valid registration.
                ext.register(cx, component, &[], |_, _, _, cx| {
                    cx.new(|_| gpui::Empty).into()
                });
            }
            ext.place_root(cx, "root", "tour.card");
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
                ext.place(cx, node, component, parent, slot);
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
            assert_eq!(
                root.nodes("body")
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                ["left", "right"]
            );
            assert_eq!(
                handles["left"]
                    .nodes("body")
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                ["inner"]
            );
            assert_eq!(
                root.nodes("other")
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                ["last"]
            );
            assert_eq!(left_body.len(), 1);
            assert_eq!(other.len(), 1);
            for (view, again) in root_body.iter().zip(root.views("body")) {
                assert_eq!(view.entity_id(), again.entity_id());
            }
            assert_eq!(
                left_body[0].entity_id(),
                handles["left"].views("body")[0].entity_id()
            );
            assert_eq!(other[0].entity_id(), root.views("other")[0].entity_id());
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
        "node-place: DEBUG message=node placed node=left component=tour.card extension=tour parent=root slot=body",
        "component-create: ERROR message=node missing names unregistered component tour.missing",
        concat!(
            "component-create: ERROR message=node aside is placed in slot ",
            "side not declared by parent component tour.card",
        ),
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
    let root = " node=root component=tour.card extension=tour";
    let left = " node=left component=tour.card extension=tour";
    assert_eq!(
        captured.parents,
        [
            (root.into(), None),
            (left.into(), Some(root.into())),
            (
                " node=inner component=tour.card extension=tour".into(),
                Some(left.into())
            ),
            (
                " node=missing component=tour.missing extension=tour".into(),
                Some(root.into())
            ),
            (
                " node=right component=tour.card extension=tour".into(),
                Some(root.into())
            ),
            (
                " node=aside component=tour.card extension=tour".into(),
                Some(root.into())
            ),
            (
                " node=last component=tour.card extension=tour".into(),
                Some(root.into())
            ),
        ]
    );
}

fn extensions_contract(cx: &mut TestAppContext) {
    let trace = Trace::default();
    let created = Rc::new(RefCell::new(Vec::new()));
    let handles = Rc::new(RefCell::new(BTreeMap::new()));
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(riso_ui::init);
        app.update(|cx| {
            let _group = tracing::info_span!("extension-init", extension = "tour").entered();
            let host = riso_shell::Extension::new(cx, "tour").unwrap();
            for invalid in ["", "Tour", "0tour", "tour.bad", "tour name", "tøur"] {
                assert!(riso_shell::Extension::new(cx, invalid).is_none());
            }
            assert!(riso_shell::Extension::new(cx, "tour").is_none());
            let guest = riso_shell::Extension::new(cx, "tour_extra").unwrap();
            host.register(cx, "tour_extra.card", &[], |_, _, _, _| {
                panic!("foreign registration accepted")
            });
            guest.register(cx, "tour.card", &[], |_, _, _, _| {
                panic!("foreign registration accepted")
            });
            // A foreign root must not occupy the root or reserve its node ID.
            guest.place_root(cx, "root", "tour.card");
            let saved_created = created.clone();
            let saved_handles = handles.clone();
            host.clone()
                .register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
                    saved_created.borrow_mut().push(node.to_string());
                    saved_handles.borrow_mut().insert(node.to_string(), slots);
                    cx.new(|_| gpui::Empty).into()
                });
            host.clone().place_root(cx, "root", "tour.card");
            let saved_created = created.clone();
            let saved_handles = handles.clone();
            guest.register(
                cx,
                "tour_extra.card",
                &["body"],
                move |node, slots, _, cx| {
                    saved_created.borrow_mut().push(node.to_string());
                    saved_handles.borrow_mut().insert(node.to_string(), slots);
                    cx.new(|_| gpui::Empty).into()
                },
            );
            // A foreign child must leave the node ID free for a valid contribution.
            guest.place(cx, "guest", "tour.card", "root", "body");
            host.place(cx, "prefix", "tour_extra.card", "root", "body");
            guest
                .clone()
                .place(cx, "guest", "tour_extra.card", "root", "body");
            host.place(cx, "own", "tour.card", "guest", "body");
            // Node IDs remain global across extensions.
            guest.place(cx, "own", "tour_extra.card", "root", "body");
            host.place(cx, "guest", "tour.card", "root", "body");
        });
        let window = open(&app);
        assert_eq!(*created.borrow(), ["root", "guest", "own"]);
        assert_eq!(handles.borrow()["root"].views("body").len(), 1);
        assert_eq!(handles.borrow()["guest"].views("body").len(), 1);
        for _ in 0..2 {
            gpui::AnyWindowHandle::from(window)
                .update(&mut app, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
        }
        assert_eq!(*created.borrow(), ["root", "guest", "own"]);
        // A guest loaded before its host is refused without reserving the ID.
        let early = cx.new_app();
        early.update(riso_ui::init);
        early.update(|cx| {
            let guest = riso_shell::Extension::new(cx, "tour_extra").unwrap();
            guest.register(cx, "tour_extra.card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            guest.place(cx, "early", "tour_extra.card", "root", "body");
        });
        let _window = open(&early);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    for fragment in [
        "extension-init: ERROR message=invalid extension name",
        "extension-init: ERROR message=extension tour is already initialized",
        concat!(
            "component-register: ERROR message=extension tour cannot register ",
            "component tour_extra.card",
        ),
        concat!(
            "component-register: ERROR message=extension tour_extra cannot ",
            "register component tour.card",
        ),
        "root-place: ERROR message=extension tour_extra cannot place component tour.card",
        "node-place: ERROR message=extension tour_extra cannot place component tour.card",
        "node-place: ERROR message=extension tour cannot place component tour_extra.card",
        "node-place: INFO message=the node ID is already in the tree node=own",
        "node-place: INFO message=the node ID is already in the tree node=guest",
        "node-place: INFO message=the parent is not in the tree node=early",
        "INFO component-create node=guest component=tour_extra.card extension=tour_extra",
        "INFO component-create node=own component=tour.card extension=tour",
        "INFO shell-create root=none",
    ] {
        assert!(
            captured.lines.iter().any(|line| line.contains(fragment)),
            "missing {fragment:?} in {:#?}",
            captured.lines
        );
    }
    let root = " node=root component=tour.card extension=tour";
    let guest = " node=guest component=tour_extra.card extension=tour_extra";
    assert!(
        captured
            .parents
            .contains(&(guest.into(), Some(root.into())))
    );
    assert!(captured.parents.contains(&(
        " node=own component=tour.card extension=tour".into(),
        Some(guest.into())
    )));
}
