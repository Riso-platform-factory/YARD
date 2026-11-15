#![cfg(feature = "dev")]

// What the dev shell remembers about emptiness across operations: tab
// selection after a reset, containers that lose their children, views that a
// reset drops, a zoomed node that becomes empty, and one read per node and
// frame.

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_layout::NodeId;
use riso_shell::dev as shell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

// Every test captures the lifecycle trace and runs under this one lock, so no
// thread reaches the shell's log lines without a subscriber.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

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
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", event.metadata().level(), fields.0).replace('"', ""));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

impl Trace {
    fn mark(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    fn since(&self, mark: usize) -> Vec<String> {
        self.0.lock().unwrap()[mark..].to_vec()
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
}

struct Card {
    node: String,
    empty: bool,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

impl shell::Emptiable for Card {
    fn is_empty(&self, _: &gpui::App) -> bool {
        self.empty
    }
}

type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

fn setup(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let host = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                empty: false,
            });
            created
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        host.register_empty::<Card>(cx, "tour.card");
        host
    });
    (host, views)
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}

// Changes a view's state and notifies, without drawing.
fn set_empty(app: &mut TestAppContext, views: &Views, node: &str, empty: bool) {
    app.update(|cx| {
        views
            .borrow()
            .get(node)
            .and_then(|view| view.upgrade())
            .expect("the node has a view")
            .update(cx, |card, cx| {
                card.empty = empty;
                cx.notify();
            });
    });
}

fn changes(lines: &[String], node: &str) -> Vec<String> {
    lines
        .iter()
        .filter(|line| {
            line.contains("message=emptiness changed") && line.contains(&format!("node={node} "))
        })
        .cloned()
        .collect()
}

// A reset chooses the tab from what the views say at the next layout, not
// from what the last layout read.
#[gpui::test]
fn a_reset_selects_the_first_tab_from_fresh_emptiness(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            host.place_root(cx, "tabs", "riso.tabs");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "tabs", "children");
            }
        });
        let window = open_and_draw(&mut app);
        set_empty(&mut app, &views, "a", true);
        draw(window, &mut app);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "tabs")),
            Some(NodeId::parse("b").unwrap())
        );
        app.update(|cx| host.set_hidden(cx, "c", true));
        // `a` is no longer empty, and no frame has read it yet.
        set_empty(&mut app, &views, "a", false);
        window
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "tabs")),
            Some(NodeId::parse("a").unwrap()),
            "the reset selects the first tab"
        );
        assert!(!app.update(|cx| shell::is_hidden(cx, "c")));
    });
}

// A container that loses the child that made it empty is no longer empty,
// and says so once.
#[gpui::test]
fn a_container_that_loses_its_empty_child_is_no_longer_empty(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "left", "riso.column", "root", "children");
            host.place(cx, "l1", "tour.card", "left", "children");
            host.place(cx, "right", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let before = trace.mark();
        set_empty(&mut app, &views, "l1", true);
        draw(window, &mut app);
        let lines = trace.since(before);
        let left = changes(&lines, "left");
        assert_eq!(left.len(), 1, "{lines:#?}");
        assert!(left[0].contains("empty=true"), "{}", left[0]);
        assert!(app.update(|cx| shell::node_bounds(cx, "left")).is_none());

        let before = trace.mark();
        app.update(|cx| host.remove(cx, "l1"));
        draw(window, &mut app);
        draw(window, &mut app);
        let lines = trace.since(before);
        let left = changes(&lines, "left");
        assert_eq!(left.len(), 1, "once: {lines:#?}");
        assert!(left[0].contains("DEBUG"), "{}", left[0]);
        assert!(left[0].contains("empty=false"), "{}", left[0]);
        assert!(!app.update(|cx| shell::is_empty(cx, "left")));
        assert!(
            app.update(|cx| shell::node_bounds(cx, "left")).is_some(),
            "left is drawn again"
        );
    });
}

// A reset that drops a node added at runtime forgets its emptiness, so a node
// added again under the same ID starts afresh.
#[gpui::test]
fn a_reset_forgets_the_emptiness_of_the_nodes_it_drops(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        window
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "d", "tour.card", "root", "children")
            })
            .unwrap();
        draw(window, &mut app);
        set_empty(&mut app, &views, "d", true);
        draw(window, &mut app);
        assert!(app.update(|cx| shell::is_empty(cx, "d")));
        window
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);

        let before = trace.mark();
        window
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "d", "tour.card", "root", "children")
            })
            .unwrap();
        draw(window, &mut app);
        draw(window, &mut app);
        let lines = trace.since(before);
        assert!(
            changes(&lines, "d").is_empty(),
            "the new d has no old state: {lines:#?}"
        );
        assert!(!app.update(|cx| shell::is_empty(cx, "d")));
        assert!(app.update(|cx| shell::node_bounds(cx, "d")).is_some());
    });
}

// A zoomed node that becomes empty is zoomed out of, and the focus leaves it.
#[gpui::test]
fn a_zoomed_node_that_becomes_empty_is_zoomed_out_of(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "b", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        app.update(|cx| host.set_zoomed(cx, "a"));
        draw(window, &mut app);
        let bounds = visual.debug_bounds("CARD-a").expect("a is drawn");
        visual.simulate_click(bounds.center(), Default::default());
        assert_eq!(
            visual
                .update(|window, cx| shell::focused_node(window, cx))
                .map(|node| node.to_string())
                .as_deref(),
            Some("a")
        );
        let before = trace.mark();
        set_empty(&mut app, &views, "a", true);
        draw(window, &mut app);
        draw(window, &mut app);
        draw(window, &mut app);
        assert_eq!(app.update(|cx| shell::zoomed(cx)), None);
        assert_eq!(
            visual.update(|window, cx| shell::focused_node(window, cx)),
            None,
            "the focus leaves the empty node"
        );
        assert!(app.update(|cx| shell::is_empty(cx, "a")));
        let b = visual.debug_bounds("CARD-b").expect("b is drawn");
        let body = app.update(|cx| shell::node_bounds(cx, "body")).unwrap();
        assert_eq!(b, body, "b takes the row");
        let lines = trace.since(before);
        assert!(
            lines.iter().any(
                |line| line.contains("message=zoomed node is empty") && line.contains("node=a")
            ),
            "{lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("message=zoomed out")),
            "{lines:#?}"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

// Nested containers read each node's emptiness once per frame.
#[gpui::test]
fn a_nested_node_is_read_once_per_frame(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, _views) = setup(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "outer", "riso.column", "root", "children");
            host.place(cx, "inner", "riso.row", "outer", "children");
            host.place(cx, "leaf", "tour.card", "inner", "children");
            host.place(cx, "side", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let before = trace.mark();
        draw(window, &mut app);
        let lines = trace.since(before);
        for node in ["leaf", "inner", "outer"] {
            let reads = lines
                .iter()
                .filter(|line| {
                    line.starts_with("TRACE")
                        && line.contains("message=empty ")
                        && line.contains(&format!("node={node} "))
                })
                .count();
            assert_eq!(reads, 1, "{node} is read once in a frame: {lines:#?}");
        }
    });
}
