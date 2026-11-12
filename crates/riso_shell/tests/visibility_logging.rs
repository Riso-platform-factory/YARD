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
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            && !event.metadata().target().starts_with("riso_layout")
            && event.metadata().target() != "visibility_diagnostic"
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
            tracing::error!(target: "visibility_diagnostic", "{}", record.args());
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

fn widths(shown: &Shown, expected: &[(&str, f32)]) {
    let shown = shown.borrow();
    assert_eq!(shown.len(), expected.len(), "{shown:?}");
    for ((node, bounds), (expected_node, width)) in shown.iter().zip(expected) {
        assert_eq!(node, expected_node);
        assert_eq!(bounds.size.width, gpui::px(*width));
    }
}

fn shown_nodes(shown: &Shown) -> Vec<String> {
    shown
        .borrow()
        .iter()
        .map(|(node, _)| node.clone())
        .collect()
}

fn register(cx: &mut gpui::App, ext: &riso_shell::Extension, shown: &Shown) {
    let shown = shown.clone();
    ext.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
        cx.new(|_| Card {
            node: node.to_string(),
            shown: shown.clone(),
            slots,
        })
        .into()
    });
}

// Alone in this binary: all captured log lines run under this subscriber.
#[gpui::test]
fn visibility_retains_views_shares_and_selection_and_logs_each_result(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        slots_contract(cx);
        splits_contract(cx);
        tabs_contract(cx, &trace);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    for line in [
        "node-visibility node=\"b\" hidden=true extension=\"guest\": DEBUG message=node hidden node=b",
        "node-visibility node=\"b\" hidden=false extension=\"guest\": DEBUG message=node shown node=b",
        "node-visibility node=\"b\" hidden=false extension=\"guest\": DEBUG message=already in that state node=b hidden=false",
        "node-visibility node=\"root\" hidden=true extension=\"guest\": INFO message=the root cannot be hidden or shown node=root",
        "node-visibility node=\"root\" hidden=false extension=\"guest\": INFO message=the root cannot be hidden or shown node=root",
        "node-visibility node=\"missing\" hidden=true extension=\"guest\": INFO message=the node is not in the tree node=missing",
        "node-visibility node=\"missing\" hidden=false extension=\"guest\": INFO message=the node is not in the tree node=missing",
        "node-visibility node=\"Bad\" hidden=true extension=\"guest\": ERROR message=invalid node ID \"Bad\": a slug must start with a lowercase ASCII letter",
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
        1
    );
}

fn slots_contract(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let slots = Rc::new(RefCell::new(BTreeMap::new()));
    let created = Rc::new(RefCell::new(BTreeMap::new()));
    let (host, guest) = app.update(|cx| {
        let host = riso_shell::Extension::new(cx, "tour").unwrap();
        let guest = riso_shell::Extension::new(cx, "guest").unwrap();
        let slots = slots.clone();
        let created = created.clone();
        let shown = shown.clone();
        host.register(cx, "tour.card", &["body"], move |node, children, _, cx| {
            slots
                .borrow_mut()
                .insert(node.to_string(), children.clone());
            let view: gpui::AnyView = cx
                .new(|_| Card {
                    node: node.to_string(),
                    shown: shown.clone(),
                    slots: children,
                })
                .into();
            created
                .borrow_mut()
                .insert(node.to_string(), view.downgrade());
            view
        });
        host.place_root(cx, "root", "tour.card");
        for (node, parent) in [("a", "root"), ("b", "root"), ("inner", "b")] {
            host.place(cx, node, "tour.card", parent, "body");
        }
        guest.set_hidden(cx, "b", true);
        (host, guest)
    });
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    assert_eq!(
        created.borrow().len(),
        4,
        "initially hidden views are created too"
    );
    let root = slots.borrow()["root"].clone();
    assert_eq!(
        root.nodes("body")
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(root.views("body").len(), 1);
    let b = created.borrow()["b"].upgrade().unwrap().entity_id();
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["root", "a"]);
    app.update(|cx| host.set_hidden(cx, "inner", true));
    app.update(|cx| guest.set_hidden(cx, "b", false));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["root", "a", "b"]);
    assert_eq!(root.views("body")[1].entity_id(), b);
    app.update(|cx| {
        assert!(riso_shell::is_hidden(cx, "inner"));
        guest.set_hidden(cx, "inner", false);
        guest.set_hidden(cx, "b", false);
        for hidden in [true, false] {
            guest.set_hidden(cx, "root", hidden);
            guest.set_hidden(cx, "missing", hidden);
        }
        guest.set_hidden(cx, "Bad", true);
    });
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["root", "a", "b", "inner"]);
    assert_eq!(root.views("body")[1].entity_id(), b);
    assert_eq!(created.borrow().len(), 4);
    assert!(
        created
            .borrow()
            .values()
            .all(|view| view.upgrade().is_some())
    );
}

fn drag(visual: &mut gpui::VisualTestContext, from: f32, to: f32) {
    let position = |x| gpui::point(gpui::px(x), gpui::px(100.));
    visual.simulate_mouse_move(position(from), None, Default::default());
    visual.simulate_mouse_down(position(from), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(position(to), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(position(to), gpui::MouseButton::Left, Default::default());
}

fn splits_contract(cx: &mut TestAppContext) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let ext = app.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        register(cx, &ext, &shown);
        ext.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
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
    widths(&shown, &[("a", 200.), ("b", 200.), ("c", 200.)]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    drag(&mut visual, 200., 300.);
    // No explicit render between changing the vector and hiding its middle ID.
    app.update(|cx| ext.set_hidden(cx, "b", true));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 360.), ("c", 240.)]);
    app.update(|cx| ext.set_hidden(cx, "b", false));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 300.), ("b", 100.), ("c", 200.)]);
    app.update(|cx| ext.set_hidden(cx, "a", true));
    draw(window, &mut app, &shown);
    widths(&shown, &[("b", 200.), ("c", 400.)]);
    drag(&mut visual, 200., 300.);
    app.update(|cx| ext.set_hidden(cx, "a", false));
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 300.), ("b", 150.), ("c", 150.)]);
    app.update(|cx| {
        for node in ["a", "b", "c"] {
            ext.set_hidden(cx, node, true);
        }
    });
    draw(window, &mut app, &shown);
    widths(&shown, &[]);
    app.update(|cx| ext.set_hidden(cx, "b", false));
    draw(window, &mut app, &shown);
    widths(&shown, &[("b", 600.)]);
    app.update(|cx| {
        for node in ["a", "c"] {
            ext.set_hidden(cx, node, false);
        }
    });
    draw(window, &mut app, &shown);
    widths(&shown, &[("a", 300.), ("b", 150.), ("c", 150.)]);
}

fn tabs_contract(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let ext = app.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        register(cx, &ext, &shown);
        ext.place_root(cx, "root", "riso.tabs");
        for node in ["a", "b", "c"] {
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
    assert_eq!(shown_nodes(&shown), ["a"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let bounds = visual.debug_bounds("TAB-c").unwrap();
    visual.simulate_click(bounds.center(), Default::default());
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["c"]);
    app.update(|cx| ext.set_hidden(cx, "a", true));
    draw(window, &mut app, &shown);
    assert_eq!(
        shown_nodes(&shown),
        ["c"],
        "selection follows ID after earlier tab disappears"
    );
    assert!(visual.debug_bounds("TAB-a").is_none());
    app.update(|cx| ext.set_hidden(cx, "c", true));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["b"]);
    app.update(|cx| ext.set_hidden(cx, "c", false));
    draw(window, &mut app, &shown);
    assert_eq!(
        shown_nodes(&shown),
        ["b"],
        "show does not restore the old selection"
    );
    let bounds = visual.debug_bounds("TAB-c").unwrap();
    visual.simulate_click(bounds.center(), Default::default());
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["c"]);
    app.update(|cx| {
        for node in ["b", "c"] {
            ext.set_hidden(cx, node, true);
        }
    });
    draw(window, &mut app, &shown);
    assert!(shown.borrow().is_empty());
    app.update(|cx| ext.set_hidden(cx, "a", false));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a"]);
    let captured = trace.0.lock().unwrap();
    let selected: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tab selected"))
        .collect();
    assert_eq!(
        selected.len(),
        2,
        "fallbacks must not log tab selected: {selected:?}"
    );
    assert!(
        selected
            .iter()
            .all(|line| line.as_str() == "none: DEBUG message=tab selected tabs=root tab=c")
    );
}
