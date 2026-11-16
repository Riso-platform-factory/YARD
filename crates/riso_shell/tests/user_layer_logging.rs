use gpui::{AppContext, IntoElement, ParentElement, Styled, TestAppContext, px};
use riso_shell as shell;
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

type Shown = Rc<RefCell<Vec<(String, gpui::Bounds<gpui::Pixels>)>>>;

struct Card {
    node: String,
    marker: usize,
    slots: shell::Slots,
    shown: Shown,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
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
            .children(
                self.slots
                    .views("body")
                    .into_iter()
                    .map(|view| gpui::div().flex_1().min_h_0().child(view)),
            )
    }
}

// Draw without forcing a refresh: operations and reset must schedule it themselves.
fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn nodes(shown: &Shown) -> Vec<String> {
    shown
        .borrow()
        .iter()
        .map(|(node, _)| node.clone())
        .collect()
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
}

fn drag(
    visual: &mut gpui::VisualTestContext,
    from: gpui::Point<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
) {
    visual.simulate_mouse_move(from, None, Default::default());
    visual.simulate_mouse_down(from, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
}

// Alone in this binary: every call reaching captured lines uses this subscriber.
#[gpui::test]
fn user_layer_records_and_reset_reconciles_every_contract_row(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(riso_ui::init);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        let shown = Shown::default();
        let views = Rc::new(RefCell::new(BTreeMap::new()));
        let created = Rc::new(RefCell::new(Vec::new()));
        let (host, guest) = app.update(|cx| {
            let host = shell::Extension::new(cx, "tour").unwrap();
            let guest = shell::Extension::new(cx, "guest").unwrap();
            let saved_views = views.clone();
            let saved_created = created.clone();
            let factory_host = host.clone();
            let shown = shown.clone();
            host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
                factory_host.place(cx, "factory-late", "tour.card", "root", "children");
                saved_created.borrow_mut().push(node.to_string());
                let card = cx.new(|_| Card {
                    node: node.to_string(),
                    marker: 0,
                    slots,
                    shown: shown.clone(),
                });
                saved_views
                    .borrow_mut()
                    .insert(node.to_string(), card.downgrade());
                card.into()
            });
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "inner", "tour.card", "a", "body");
            host.place(cx, "left", "riso.tabs", "root", "children");
            host.place(cx, "right", "riso.tabs", "root", "children");
            for (node, parent) in [("b", "left"), ("c", "left"), ("d", "right"), ("e", "right")] {
                host.place(cx, node, "tour.card", parent, "children");
            }
            // The default is fixed at window open, including init-time operations.
            host.set_hidden(cx, "e", true);
            assert_eq!(shell::user_layer_len(cx), 1);
            (host, guest)
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        let any_window = gpui::AnyWindowHandle::from(window);
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        window
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        let original: BTreeMap<_, _> = views
            .borrow()
            .iter()
            .map(|(node, view)| (node.clone(), view.upgrade().unwrap().entity_id()))
            .collect();
        app.update(|cx| {
            for node in ["a", "inner", "b", "c", "d", "e"] {
                views.borrow()[node]
                    .upgrade()
                    .unwrap()
                    .update(cx, |card, _| card.marker = 42);
            }
        });

        // A drag records one set-props; tab selection and zoom stay view state.
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        drag(
            &mut visual,
            gpui::point(px(300.), px(300.)),
            gpui::point(px(400.), px(300.)),
        );
        draw(window, &mut app, &shown);
        assert_eq!(shown.borrow()[0].1.size.width, px(400.));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        click(&mut visual, "TAB-c");
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "c", "d"]);
        app.update(|cx| host.set_zoomed(cx, "c"));
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["c"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        let creates = created.borrow().len();
        let before_reset = trace.0.lock().unwrap().lines.len();
        any_window
            .update(&mut app, |_, window, cx| guest.reset_layout(window, cx))
            .unwrap();
        assert_eq!(app.update(|cx| shell::zoomed(cx)), None);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(created.borrow().len(), creates);
        let reset_lines = trace.0.lock().unwrap().lines[before_reset..].to_vec();
        for line in [
            r#"layout-reset extension="guest": DEBUG message=user layer dropped dropped=1"#,
            "props-read node=\"root\" component=\"riso.row\": DEBUG message=props read keys=\"\"",
            r#"layout-reset extension="guest": DEBUG message=view told node=root"#,
        ] {
            assert!(
                reset_lines.iter().any(|actual| actual == line),
                "missing {line} in {reset_lines:#?}"
            );
        }
        // The dropped set-props restores equal shares; zoom and selection clear too.
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);
        for node in ["a", "b", "d"] {
            assert_eq!(
                shown
                    .borrow()
                    .iter()
                    .find(|(id, _)| id == node)
                    .unwrap()
                    .1
                    .size
                    .width,
                px(300.)
            );
        }

        // A real tab drag uses the shell's layout-op and records exactly once.
        let start = visual.debug_bounds("TAB-b").unwrap().center();
        let target = visual.debug_bounds("TABBAR-right").unwrap().center();
        visual.simulate_mouse_move(start, None, Default::default());
        visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            gpui::point(start.x + px(5.), start.y),
            gpui::MouseButton::Left,
            Default::default(),
        );
        draw(window, &mut app, &shown);
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        draw(window, &mut app, &shown);
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        draw(window, &mut app, &shown);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        assert!(visual.debug_bounds("TAB-b").unwrap().origin.x > px(600.));
        // Refused model and shell operations never enter the layer.
        let before_refusals = trace.0.lock().unwrap().lines.len();
        any_window
            .update(&mut app, |_, window, cx| {
                guest.remove(cx, "root");
                host.add(window, cx, "bad-slot", "tour.card", "a", "side");
                host.set_hidden(cx, "missing", true);
                assert_eq!(shell::user_layer_len(cx), 1);
            })
            .unwrap();
        assert!(
            !trace.0.lock().unwrap().lines[before_refusals..]
                .iter()
                .any(|line| line.contains("recorded"))
        );

        any_window
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "added", "tour.card", "root", "children");
                host.add(window, cx, "sub", "tour.card", "added", "body");
                guest.remove(cx, "a");
                guest.set_hidden(cx, "d", true);
                guest.set_hidden(cx, "e", false);
                assert_eq!(shell::user_layer_len(cx), 6);
                guest.set_zoomed(cx, "c");
            })
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["c"]);
        assert!(views.borrow()["a"].upgrade().is_none());
        assert!(views.borrow()["inner"].upgrade().is_none());
        let added = views.borrow()["added"].clone();
        let sub = views.borrow()["sub"].clone();
        let before_reset = trace.0.lock().unwrap().lines.len();
        created.borrow_mut().clear();
        any_window
            .update(&mut app, |_, window, cx| guest.reset_layout(window, cx))
            .unwrap();
        // No explicit refresh: reset must invalidate the window.
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(app.update(|cx| shell::zoomed(cx)), None);
        assert!(app.update(|cx| shell::is_hidden(cx, "e")));
        assert!(!app.update(|cx| shell::is_hidden(cx, "d")));
        for node in ["a", "b", "d"] {
            assert_eq!(
                shown
                    .borrow()
                    .iter()
                    .find(|(id, _)| id == node)
                    .unwrap()
                    .1
                    .size
                    .width,
                px(300.)
            );
        }
        assert_eq!(
            *created.borrow(),
            ["a", "inner"],
            "missing views are created parent first under a kept root"
        );
        for node in ["a", "inner"] {
            let view = views.borrow()[node].upgrade().unwrap();
            assert_ne!(view.entity_id(), original[node]);
            assert_eq!(app.update(|cx| view.read(cx).marker), 0);
        }
        for node in ["b", "c", "d", "e"] {
            let view = views.borrow()[node].upgrade().unwrap();
            assert_eq!(view.entity_id(), original[node], "kept {node} entity");
            assert_eq!(app.update(|cx| view.read(cx).marker), 42);
        }
        assert!(added.upgrade().is_none());
        assert!(sub.upgrade().is_none());
        let reset_lines = trace.0.lock().unwrap().lines[before_reset..].to_vec();
        for line in [
            r#"layout-reset extension="guest": DEBUG message=user layer dropped dropped=6"#,
            r#"layout-reset extension="guest": DEBUG message=view dropped node=added"#,
            r#"layout-reset extension="guest": DEBUG message=view dropped node=sub"#,
        ] {
            assert!(
                reset_lines.iter().any(|actual| actual == line),
                "missing {line} in {reset_lines:#?}"
            );
        }
        let captured = trace.0.lock().unwrap();
        for expected in [
            vec![
                "window id=4294967297",
                r#"layout-reset extension="guest""#,
                "component-create node=a component=tour.card extension=tour",
            ],
            vec![
                "window id=4294967297",
                r#"layout-reset extension="guest""#,
                "component-create node=a component=tour.card extension=tour",
                "component-create node=inner component=tour.card extension=tour",
            ],
        ] {
            assert!(
                captured.span_stacks.iter().any(|stack| stack
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    == expected),
                "missing {expected:?} in {:#?}",
                captured.span_stacks
            );
        }
        let recorded_resize = concat!(
            "layout-op op=\"props\" node=\"root\" extension=\"riso\": ",
            "DEBUG message=recorded user_layer=1",
        );
        assert!(captured.lines.iter().any(|line| line == recorded_resize));
        let recorded_drag = concat!(
            "layout-op op=\"move\" node=\"b\" extension=\"riso\": ",
            "DEBUG message=recorded user_layer=1",
        );
        assert!(captured.lines.iter().any(|line| line == recorded_drag));
        for (op, node, count) in [
            ("add", "added", 2),
            ("add", "sub", 3),
            ("remove", "a", 4),
            ("hide", "d", 5),
            ("show", "e", 6),
        ] {
            let expected = format!(
                "layout-op op={op:?} node={node:?} extension={:?}: DEBUG message=recorded user_layer={count}",
                if op == "add" { "tour" } else { "guest" }
            );
            assert!(
                captured.lines.contains(&expected),
                "missing {expected} in {:#?}",
                captured.lines
            );
        }
        drop(captured);
        // A missing descendant under a kept component must also be recreated.
        let restored_a = views.borrow()["a"].upgrade().unwrap().entity_id();
        app.update(|cx| guest.remove(cx, "inner"));
        created.borrow_mut().clear();
        any_window
            .update(&mut app, |_, window, cx| guest.reset_layout(window, cx))
            .unwrap();
        assert_eq!(*created.borrow(), ["inner"]);
        assert_eq!(
            views.borrow()["a"].upgrade().unwrap().entity_id(),
            restored_a
        );

        // Startup placement cannot mutate the fixed default or bypass the layer.
        app.update(|cx| {
            host.place(cx, "late", "tour.card", "root", "children");
            host.place_root(cx, "late-root", "tour.card");
            assert_eq!(shell::user_layer_len(cx), 0);
        });
        any_window
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "late", "tour.card", "root", "children");
                assert_eq!(
                    shell::user_layer_len(cx),
                    1,
                    "refused startup placement consumes no ID"
                );
                host.reset_layout(window, cx);
            })
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);

        for group in [
            r#"node-place node="late" component="tour.card" parent="root" slot="children""#,
            r#"root-place node="late-root" component="tour.card""#,
        ] {
            assert!(trace.0.lock().unwrap().lines.contains(&format!(
                "{group}: INFO message=the window is open; add the node with add"
            )));
        }

        // Reusing a removed default ID must not keep its user-added replacement view.
        let replacements = Rc::new(RefCell::new(Vec::new()));
        app.update(|cx| {
            let replacements = replacements.clone();
            host.register(cx, "tour.replacement", &[], move |_, _, _, cx| {
                let view: gpui::AnyView = cx.new(|_| gpui::Empty).into();
                replacements.borrow_mut().push(view.downgrade());
                view
            });
            guest.remove(cx, "a");
            // Move a kept child into the removed node's original default slot.
            guest.move_node(cx, "b", "root", "children", 0);
        });
        let replacement = any_window
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "a", "tour.replacement", "root", "children");
                assert_eq!(shell::user_layer_len(cx), 3);
                let replacement = replacements.borrow_mut().remove(0);
                host.reset_layout(window, cx);
                replacement
            })
            .unwrap();
        draw(window, &mut app, &shown);
        assert!(replacement.upgrade().is_none());
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);
        assert_ne!(
            views.borrow()["a"].upgrade().unwrap().entity_id(),
            restored_a
        );
        assert_eq!(
            views.borrow()["b"].upgrade().unwrap().entity_id(),
            original["b"]
        );
        // Recreating a parent must preserve a child that survived elsewhere.
        let inner = views.borrow()["inner"].upgrade().unwrap().entity_id();
        app.update(|cx| {
            guest.move_node(cx, "inner", "root", "children", 0);
            guest.remove(cx, "a");
        });
        any_window
            .update(&mut app, |_, window, cx| guest.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(
            views.borrow()["inner"].upgrade().unwrap().entity_id(),
            inner
        );
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);

        // Opening another shell must preserve the first startup default and layer.
        app.update(|cx| guest.set_hidden(cx, "b", true));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        let second = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        second
            .update(&mut app, |_, window, cx| guest.reset_layout(window, cx))
            .unwrap();
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        draw(second, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "inner", "b", "d"]);
    });
    assert!(trace.0.lock().unwrap().entered.is_empty());
}
