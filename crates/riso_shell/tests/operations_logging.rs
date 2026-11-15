use gpui::{AppContext, IntoElement, Styled, TestAppContext};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
    span_stacks: Vec<Vec<String>>,
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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        if attributes.metadata().target().starts_with("riso_shell") {
            let mut stack: Vec<String> = captured
                .entered
                .iter()
                .map(|entered| captured.spans[entered].clone())
                .collect();
            stack.push(label.clone());
            captured.span_stacks.push(stack);
        }
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            && !event.metadata().target().starts_with("riso_layout")
            && event.metadata().target() != "operations_diagnostic"
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        captured.lines.push(line);
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
            tracing::error!(target: "operations_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

type Shown = Rc<RefCell<Vec<(String, gpui::Bounds<gpui::Pixels>)>>>;

struct Card {
    node: String,
    shown: Shown,
    slots: riso_shell::Slots,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::ParentElement;
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::canvas(
                    move |bounds, _, _| shown.borrow_mut().push((node, bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(self.slots.views("body"))
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, cx: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn shown_nodes(shown: &Shown) -> Vec<String> {
    shown
        .borrow()
        .iter()
        .map(|(node, _)| node.clone())
        .collect()
}

fn widths(shown: &Shown, expected: &[(&str, f32)]) {
    let shown = shown.borrow();
    assert_eq!(shown.len(), expected.len(), "{shown:?}");
    for ((node, bounds), (expected_node, width)) in shown.iter().zip(expected) {
        assert_eq!(node, expected_node);
        assert_eq!(bounds.size.width, gpui::px(*width));
    }
}

fn drag(visual: &mut gpui::VisualTestContext, from: f32, to: f32) {
    let position = |x| gpui::point(gpui::px(x), gpui::px(100.));
    visual.simulate_mouse_move(position(from), None, Default::default());
    visual.simulate_mouse_down(position(from), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(position(to), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(position(to), gpui::MouseButton::Left, Default::default());
}

// Alone in this binary: all captured log lines run under this subscriber.
#[gpui::test]
fn operations_reconcile_views_slots_containers_and_logs(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        lifecycle_and_refusals_contract(cx);
        splits_and_visibility_contract(cx);
        tabs_and_visibility_contract(cx, &trace);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());

    // Added views are created inside layout-op > component-create.
    for stack in [
        vec![
            r#"layout-op op="add" node="d" extension="tour""#.into(),
            "component-create node=d component=tour.card extension=tour".into(),
        ],
        vec![
            r#"layout-op op="add" node="extra_row" extension="guest""#.into(),
            "component-create node=extra_row component=riso.row extension=guest".into(),
        ],
        vec![
            r#"layout-op op="add" node="probe" extension="guest""#.into(),
            "component-create node=probe component=guest.probe extension=guest".into(),
        ],
    ] {
        assert!(
            captured.span_stacks.contains(&stack),
            "missing {stack:?} in {:#?}",
            captured.span_stacks
        );
    }

    for line in [
        // Applied operations: add, move, hide, show, remove + view drops.
        r#"layout-op op="add" node="d" extension="tour": DEBUG message=node added node=d component=tour.card extension=tour parent=root slot=children"#,
        r#"layout-op op="add" node="extra_row" extension="guest": DEBUG message=node added node=extra_row component=riso.row extension=guest parent=root slot=children"#,
        r#"component-create node=extra_row component=riso.row extension=guest: DEBUG message=split created axis="row" children=0"#,
        r#"layout-op op="move" node="b" extension="guest": DEBUG message=node moved node=b parent=root slot=children index=3"#,
        r#"layout-op op="hide" node="b" extension="guest": DEBUG message=node hidden node=b"#,
        r#"layout-op op="show" node="b" extension="guest": DEBUG message=node shown node=b"#,
        r#"layout-op op="show" node="b" extension="guest": DEBUG message=already in that state node=b hidden=false"#,
        r#"layout-op op="remove" node="right" extension="guest": DEBUG message=node removed node=right removed=2"#,
        r#"layout-op op="remove" node="right" extension="guest": DEBUG message=view dropped node=right"#,
        r#"layout-op op="remove" node="right" extension="guest": DEBUG message=view dropped node=c"#,
        // Shell refusals (INFO).
        r#"layout-op op="add" node="unreg" extension="tour": INFO message=component is not registered node=unreg component=tour.missing"#,
        r#"layout-op op="add" node="builtin_unreg" extension="tour": INFO message=component is not registered node=builtin_unreg component=riso.missing"#,
        r#"layout-op op="add" node="foreign" extension="guest": INFO message=component is not in the extension's namespace node=foreign component=tour.card extension=guest"#,
        r#"layout-op op="add" node="bad_slot_add" extension="tour": INFO message=parent component does not declare slot node=bad_slot_add parent=a parent_component=tour.card slot=side"#,
        r#"layout-op op="move" node="c" extension="tour": INFO message=parent component does not declare slot node=c parent=b parent_component=tour.card slot=side"#,
        // Model refusals (INFO).
        r#"layout-op op="add" node="d" extension="tour": INFO message=the node ID is already in the tree node=d component=tour.card extension=tour parent=root slot=children"#,
        r#"layout-op op="add" node="orphan" extension="tour": INFO message=the parent is not in the tree node=orphan component=tour.card extension=tour parent=nowhere slot=body"#,
        r#"layout-op op="remove" node="missing" extension="tour": INFO message=the node is not in the tree node=missing"#,
        r#"layout-op op="remove" node="root" extension="tour": INFO message=the root cannot be removed node=root"#,
        r#"layout-op op="move" node="missing" extension="tour": INFO message=the node is not in the tree node=missing parent=root slot=children index=0"#,
        r#"layout-op op="move" node="root" extension="tour": INFO message=the root cannot be moved node=root parent=a slot=body index=0"#,
        r#"layout-op op="move" node="a" extension="tour": INFO message=the parent is not in the tree node=a parent=nowhere slot=body index=0"#,
        r#"layout-op op="move" node="right" extension="tour": INFO message=moving a node into itself or its subtree would make a cycle node=right parent=right slot=children index=0"#,
        r#"layout-op op="move" node="right" extension="tour": INFO message=moving a node into itself or its subtree would make a cycle node=right parent=c slot=body index=0"#,
        r#"layout-op op="hide" node="missing" extension="tour": INFO message=the node is not in the tree node=missing"#,
        r#"layout-op op="show" node="missing" extension="tour": INFO message=the node is not in the tree node=missing"#,
        r#"layout-op op="hide" node="root" extension="tour": INFO message=the root cannot be hidden or shown node=root"#,
        r#"layout-op op="show" node="root" extension="tour": INFO message=the root cannot be hidden or shown node=root"#,
        // Invalid slug errors (ERROR).
        r#"layout-op op="add" node="Bad" extension="tour": ERROR message=invalid node ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="add" node="ok" extension="tour": ERROR message=invalid component ID "Bad.Card": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="add" node="ok" extension="tour": ERROR message=invalid parent ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="add" node="ok" extension="tour": ERROR message=invalid slot name "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="remove" node="Bad" extension="tour": ERROR message=invalid node ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="move" node="Bad" extension="tour": ERROR message=invalid node ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="move" node="a" extension="tour": ERROR message=invalid parent ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="move" node="a" extension="tour": ERROR message=invalid slot name "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="hide" node="Bad" extension="tour": ERROR message=invalid node ID "Bad": a slug must start with a lowercase ASCII letter"#,
    ] {
        assert!(
            captured.lines.iter().any(|actual| actual == line),
            "missing {line:?} in {:#?}",
            captured.lines
        );
    }
    assert_eq!(
        captured
            .lines
            .iter()
            .filter(|line| line.contains(": ERROR"))
            .count(),
        9
    );
}

fn lifecycle_and_refusals_contract(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let slots_by_node = Rc::new(RefCell::new(BTreeMap::new()));
    let weak_views = Rc::new(RefCell::new(BTreeMap::new()));
    let probe_windows = Rc::new(RefCell::new(BTreeMap::new()));

    let (host, guest) = app.update(|cx| {
        let host = riso_shell::Extension::new(cx, "tour").unwrap();
        let guest = riso_shell::Extension::new(cx, "guest").unwrap();
        let shown = shown.clone();
        let card_slots = slots_by_node.clone();
        let card_weaks = weak_views.clone();
        host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            card_slots
                .borrow_mut()
                .insert(node.to_string(), slots.clone());
            let view: gpui::AnyView = cx
                .new(|_| Card {
                    node: node.to_string(),
                    shown: shown.clone(),
                    slots,
                })
                .into();
            card_weaks
                .borrow_mut()
                .insert(node.to_string(), view.downgrade());
            view
        });
        let probe_weaks = weak_views.clone();
        let probe_wins = probe_windows.clone();
        guest.register(cx, "guest.probe", &[], move |node, _, window, cx| {
            probe_wins
                .borrow_mut()
                .insert(node.to_string(), window.window_handle());
            let view: gpui::AnyView = cx.new(|_| gpui::Empty).into();
            probe_weaks
                .borrow_mut()
                .insert(node.to_string(), view.downgrade());
            view
        });
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "right", "riso.column", "root", "children");
        host.place(cx, "b", "tour.card", "right", "children");
        host.place(cx, "c", "tour.card", "right", "children");
        (host, guest)
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    let any_window = gpui::AnyWindowHandle::from(window);
    app.simulate_window_resize(window.into(), gpui::size(gpui::px(600.), gpui::px(400.)));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "b", "c"]);

    let b_entity = weak_views.borrow()["b"].upgrade().unwrap().entity_id();

    // Add `d` and a child `sub` under `b`.
    any_window
        .update(&mut app, |_, window, cx| {
            host.add(window, cx, "d", "tour.card", "root", "children");
            host.add(window, cx, "sub", "tour.card", "b", "body");
        })
        .unwrap();
    assert_eq!(
        slots_by_node.borrow()["b"]
            .nodes("body")
            .iter()
            .map(|n| n.as_str())
            .collect::<Vec<_>>(),
        ["sub"]
    );
    let sub_entity = weak_views.borrow()["sub"].upgrade().unwrap().entity_id();
    let sub_view = slots_by_node.borrow()["b"].views("body")[0].entity_id();
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "b", "sub", "c", "d"]);

    // Move `b` (with `sub`) to `root.children` at the end, through `guest`'s handle.
    app.update(|cx| {
        guest.move_node(cx, "b", "root", "children", usize::MAX);
    });
    assert_eq!(
        slots_by_node.borrow()["b"].views("body")[0].entity_id(),
        sub_view
    );
    assert_eq!(
        weak_views.borrow()["sub"].upgrade().unwrap().entity_id(),
        sub_entity
    );
    assert_eq!(
        weak_views.borrow()["b"].upgrade().unwrap().entity_id(),
        b_entity,
        "moving keeps the exact view entity"
    );
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "c", "d", "b", "sub"]);

    // Guest adds a built-in container and its own component, then removes the subtree.
    any_window
        .update(&mut app, |_, window, cx| {
            guest.add(window, cx, "extra_row", "riso.row", "root", "children");
            guest.add(window, cx, "probe", "guest.probe", "extra_row", "children");
        })
        .unwrap();
    assert!(weak_views.borrow()["probe"].upgrade().is_some());
    assert_eq!(probe_windows.borrow()["probe"], any_window);
    app.update(|cx| {
        guest.remove(cx, "extra_row");
    });
    draw(window, &mut app, &shown);
    assert!(weak_views.borrow()["probe"].upgrade().is_none());

    // Move `d` and `b` into `a.body`, checking `a`'s Slots::views directly.
    let d_entity = weak_views.borrow()["d"].upgrade().unwrap().entity_id();
    app.update(|cx| {
        host.move_node(cx, "d", "a", "body", 0);
    });
    let d_view = slots_by_node.borrow()["a"].views("body")[0].entity_id();
    app.update(|cx| host.move_node(cx, "b", "a", "body", 0));
    let a_views = slots_by_node.borrow()["a"].views("body");
    assert_eq!(a_views.len(), 2);
    assert_eq!(
        slots_by_node.borrow()["a"]
            .nodes("body")
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["b", "d"]
    );
    let b_view = a_views[0].entity_id();
    assert_eq!(a_views[1].entity_id(), d_view);
    assert_eq!(
        weak_views.borrow()["b"].upgrade().unwrap().entity_id(),
        b_entity
    );
    assert_eq!(
        weak_views.borrow()["d"].upgrade().unwrap().entity_id(),
        d_entity
    );
    drop(a_views);

    // Hide and show through the unified operation path.
    app.update(|cx| {
        guest.set_hidden(cx, "b", true);
        assert!(riso_shell::is_hidden(cx, "b"));
    });
    assert_eq!(slots_by_node.borrow()["a"].views("body").len(), 1);
    assert_eq!(
        slots_by_node.borrow()["a"].views("body")[0].entity_id(),
        d_view
    );
    app.update(|cx| {
        guest.set_hidden(cx, "b", false);
        assert!(!riso_shell::is_hidden(cx, "b"));
        guest.set_hidden(cx, "b", false);
    });
    assert_eq!(slots_by_node.borrow()["a"].views("body").len(), 2);
    assert_eq!(
        slots_by_node.borrow()["a"].views("body")[0].entity_id(),
        b_view
    );

    // Exercise every refusal (shell, model, and invalid slugs); the tree must not change.
    let snapshot_before = {
        draw(window, &mut app, &shown);
        shown_nodes(&shown)
    };
    any_window
        .update(&mut app, |_, window, cx| {
            host.add(window, cx, "unreg", "tour.missing", "root", "children");
            host.add(
                window,
                cx,
                "builtin_unreg",
                "riso.missing",
                "root",
                "children",
            );
            guest.add(window, cx, "foreign", "tour.card", "root", "children");
            host.add(window, cx, "bad_slot_add", "tour.card", "a", "side");
            host.move_node(cx, "c", "b", "side", 0);

            host.add(window, cx, "d", "tour.card", "root", "children");
            host.add(window, cx, "orphan", "tour.card", "nowhere", "body");
            host.remove(cx, "missing");
            host.remove(cx, "root");
            host.move_node(cx, "missing", "root", "children", 0);
            host.move_node(cx, "root", "a", "body", 0);
            host.move_node(cx, "a", "nowhere", "body", 0);
            host.move_node(cx, "right", "right", "children", 0);
            host.move_node(cx, "right", "c", "body", 0);
            host.set_hidden(cx, "missing", true);
            host.set_hidden(cx, "missing", false);
            host.set_hidden(cx, "root", true);
            host.set_hidden(cx, "root", false);

            host.add(window, cx, "Bad", "tour.card", "root", "children");
            host.add(window, cx, "ok", "Bad.Card", "root", "children");
            host.add(window, cx, "ok", "tour.card", "Bad", "children");
            host.add(window, cx, "ok", "tour.card", "root", "Bad");
            host.remove(cx, "Bad");
            host.move_node(cx, "Bad", "root", "children", 0);
            host.move_node(cx, "a", "Bad", "children", 0);
            host.move_node(cx, "a", "root", "Bad", 0);
            host.set_hidden(cx, "Bad", true);
        })
        .unwrap();
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), snapshot_before);

    // Remove `right`, dropping `right` and `c`'s views while keeping `a`, `b`, `sub`, `d`.
    app.update(|cx| {
        guest.remove(cx, "right");
    });
    draw(window, &mut app, &shown);
    assert!(weak_views.borrow()["c"].upgrade().is_none());
    for alive in ["a", "b", "sub", "d"] {
        assert!(
            weak_views.borrow()[alive].upgrade().is_some(),
            "{alive} must remain alive"
        );
    }

    // Remove `b`, dropping both `b` and its child `sub`.
    slots_by_node.borrow_mut().remove("b");
    slots_by_node.borrow_mut().remove("sub");
    app.update(|cx| {
        host.remove(cx, "b");
    });
    draw(window, &mut app, &shown);
    assert!(weak_views.borrow()["b"].upgrade().is_none());
    assert!(weak_views.borrow()["sub"].upgrade().is_none());
    assert_eq!(shown_nodes(&shown), ["a", "d"]);
}

fn splits_and_visibility_contract(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let ext = app.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        let shown = shown.clone();
        ext.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                shown: shown.clone(),
                slots,
            })
            .into()
        });
        ext.place_root(cx, "root", "riso.row");
        for node in ["a", "b"] {
            ext.place(cx, node, "tour.card", "root", "children");
        }
        ext
    });
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(gpui::px(600.), gpui::px(400.)));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 300.), ("b", 300.)]);

    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // Resize `a` and `b` from [1.0, 1.0] to [1.5, 0.5] ([450, 150]).
    drag(&mut visual, 300., 450.);
    // Hiding `b` preserves its 0.5 share when shown again.
    app.update(|cx| ext.set_hidden(cx, "b", true));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 600.)]);
    app.update(|cx| ext.set_hidden(cx, "b", false));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 450.), ("b", 150.)]);

    // Moving `b` out of `root` keeps `b`'s entry in `root`'s `shares` prop, so
    // moving `b` back gives it its share again -> shares [1.5, 0.5] = [450, 150].
    app.update(|cx| {
        ext.move_node(cx, "b", "a", "body", 0);
        ext.move_node(cx, "b", "root", "children", usize::MAX);
    });
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 450.), ("b", 150.)]);
}

fn tabs_and_visibility_contract(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let ext = app.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        let shown = shown.clone();
        ext.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                shown: shown.clone(),
                slots,
            })
            .into()
        });
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "tabs", "riso.tabs", "root", "children");
        ext.place(cx, "park", "riso.column", "root", "children");
        for node in ["a", "b", "c"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
        ext
    });
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(gpui::px(600.), gpui::px(400.)));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a"]);

    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let click_tab = |visual: &mut gpui::VisualTestContext, selector| {
        let bounds = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(bounds.center(), Default::default());
    };
    click_tab(&mut visual, "TAB-c");
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["c"]);

    // Hide `a`; `c` stays selected.
    app.update(|cx| ext.set_hidden(cx, "a", true));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["c"]);

    // Move selected `c` out of `tabs` into `park`: `tabs` falls back to first shown (`b`).
    app.update(|cx| ext.move_node(cx, "c", "park", "children", 0));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["b", "c"]);

    // Show `a`: `b` stays selected until `a` is clicked; removing `b` falls back to `a`.
    app.update(|cx| ext.set_hidden(cx, "a", false));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["b", "c"]);
    app.update(|cx| ext.remove(cx, "b"));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "c"]);

    let captured = trace.0.lock().unwrap();
    let selected: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tab selected"))
        .collect();
    assert_eq!(
        selected,
        [&String::from(
            "none: DEBUG message=tab selected tabs=tabs tab=c"
        )]
    );
}
