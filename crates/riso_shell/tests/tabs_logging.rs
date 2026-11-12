#![cfg(feature = "dev")]

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
        if event.metadata().target() != "riso_shell::dev::shell"
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
    shown: Shown,
}
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::canvas(
            move |bounds, _, _| shown.borrow_mut().push((node, bounds)),
            |_, _, _, _| {},
        )
        .size_full()
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::dev::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn tabs_keep_their_views_and_log_only_selection_changes(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let trace = Trace::default();
    let shown = Rc::new(RefCell::new(Vec::new()));
    let views = Rc::new(RefCell::new(Vec::new()));
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            let guest = riso_shell::dev::Extension::new(cx, "guest").unwrap();
            let shown = shown.clone();
            let views = views.clone();
            host.register(cx, "tour.card", &[], move |node, slots, _, cx| {
                assert!(slots.nodes("children").is_empty());
                let view: gpui::AnyView = cx
                    .new(|_| Card {
                        node: node.to_string(),
                        shown: shown.clone(),
                    })
                    .into();
                views.borrow_mut().push(view.downgrade());
                view
            });
            // A different extension can place the built-in without registering it.
            guest.place_root(cx, "root", "riso.tabs");
            host.place(cx, "missing", "tour.missing", "root", "children");
            host.place(cx, "hidden", "tour.card", "missing", "children");
            host.place(cx, "wrong-slot", "tour.card", "root", "side");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        shown.borrow_mut().clear();
        draw(window, cx);
        assert_eq!(views.borrow().len(), 3);
        let first = views.borrow()[0].upgrade().unwrap().entity_id();
        assert_eq!(shown.borrow().last().unwrap().0, "a");
        assert!(shown.borrow().iter().all(|(node, _)| node == "a"));
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let tab_bounds = visual.debug_bounds("TAB-a").expect("first tab is rendered");
        let child_bounds = shown.borrow().last().unwrap().1;
        assert_eq!(child_bounds.size.width, gpui::px(640.));
        assert_eq!(child_bounds.origin.y, tab_bounds.bottom());
        assert_eq!(child_bounds.bottom(), gpui::px(480.));
        for (selector, node) in [("TAB-b", "b"), ("TAB-c", "c"), ("TAB-a", "a")] {
            let bounds = visual.debug_bounds(selector).expect("tab is rendered");
            visual.simulate_click(bounds.center(), Default::default());
            shown.borrow_mut().clear();
            draw(window, cx);
            assert!(!shown.borrow().is_empty());
            assert!(shown.borrow().iter().all(|(shown, _)| shown == node));
            assert!(views.borrow().iter().all(|view| view.upgrade().is_some()));
        }
        assert_eq!(views.borrow()[0].upgrade().unwrap().entity_id(), first);
        let before = trace.0.lock().unwrap().lines.clone();
        let bounds = visual.debug_bounds("TAB-a").unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        assert_eq!(trace.0.lock().unwrap().lines, before);
        assert_eq!(views.borrow().len(), 3, "switching must not create views");

        let mut nested = cx.new_app();
        nested.update(riso_ui::init);
        nested.update(|cx| {
            let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            let shown = shown.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    shown: shown.clone(),
                })
                .into()
            });
            host.place_root(cx, "split", "riso.row");
            host.place(cx, "left", "tour.card", "split", "children");
            host.place(cx, "right", "riso.tabs", "split", "children");
            host.place(cx, "b", "tour.card", "right", "children");
            host.place(cx, "c", "tour.card", "right", "children");
        });
        let window = nested.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &nested);
        for width in [640., 800.] {
            nested
                .simulate_window_resize(window.into(), gpui::size(gpui::px(width), gpui::px(480.)));
            shown.borrow_mut().clear();
            draw(window, &mut nested);
            let shown = shown.borrow();
            assert!(shown.iter().all(|(node, _)| node == "left" || node == "b"));
            let bounds = shown.iter().find(|(node, _)| node == "b").unwrap().1;
            assert_eq!(bounds.origin.x, gpui::px(width / 2.));
            assert_eq!(bounds.size.width, gpui::px(width / 2.));
            assert_eq!(bounds.bottom(), gpui::px(480.));
        }
        let bounds = visual.debug_bounds("TAB-c").unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        shown.borrow_mut().clear();
        draw(window, &mut nested);
        assert!(
            shown
                .borrow()
                .iter()
                .all(|(node, _)| node == "left" || node == "c")
        );
        assert!(shown.borrow().iter().any(|(node, _)| node == "c"));

        let mut empty = cx.new_app();
        empty.update(riso_ui::init);
        empty.update(|cx| {
            riso_shell::dev::Extension::new(cx, "guest")
                .unwrap()
                .place_root(cx, "empty", "riso.tabs");
        });
        let window = empty.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        draw(window, &mut empty);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let created: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tabs created"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        created,
        [
            "component-create node=root component=riso.tabs extension=guest: DEBUG message=tabs created tabs=3",
            "component-create node=right component=riso.tabs extension=tour: DEBUG message=tabs created tabs=2",
            "component-create node=empty component=riso.tabs extension=guest: DEBUG message=tabs created tabs=0",
        ]
    );
    let selected: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("tab selected"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        selected,
        [
            "none: DEBUG message=tab selected tabs=root tab=b",
            "none: DEBUG message=tab selected tabs=root tab=c",
            "none: DEBUG message=tab selected tabs=root tab=a",
            "none: DEBUG message=tab selected tabs=right tab=c",
        ]
    );
}
