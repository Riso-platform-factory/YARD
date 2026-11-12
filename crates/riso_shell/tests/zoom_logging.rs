#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, TestAppContext};
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
            && event.metadata().target() != "zoom_diagnostic"
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
            tracing::error!(target: "zoom_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

type Shown = Rc<RefCell<Vec<(String, gpui::Bounds<gpui::Pixels>)>>>;

struct Card {
    node: String,
    marker: usize,
    shown: Shown,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        // The shell must stretch a view that supplies no size of its own.
        gpui::canvas(
            move |bounds, _, _| shown.borrow_mut().push((node, bounds)),
            |_, _, _, _| {},
        )
    }
}

fn draw(
    window: gpui::WindowHandle<riso_shell::dev::Shell>,
    app: &mut TestAppContext,
    shown: &Shown,
) {
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

fn assert_zoomed(app: &TestAppContext, expected: Option<&str>) {
    assert_eq!(
        app.update(|cx| riso_shell::dev::zoomed(cx)),
        expected.map(str::to_owned)
    );
}

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn zoom_keeps_views_layout_and_logs_every_rule(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(riso_ui::init);
        assert_zoomed(&app, None);
        let shown = Shown::default();
        let views = Rc::new(RefCell::new(BTreeMap::new()));
        let (host, guest) = app.update(|cx| {
            let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
            let guest = riso_shell::dev::Extension::new(cx, "guest").unwrap();
            let shown = shown.clone();
            let views = views.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                let view = cx.new(|_| Card {
                    node: node.to_string(),
                    marker: 0,
                    shown: shown.clone(),
                });
                views
                    .borrow_mut()
                    .insert(node.to_string(), view.downgrade());
                view.into()
            });
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "right", "riso.tabs", "root", "children");
            for node in ["b", "c"] {
                host.place(cx, node, "tour.card", "right", "children");
            }
            guest.zoom_out(cx);
            guest.set_zoomed(cx, "missing");
            guest.set_zoomed(cx, "root");
            guest.set_zoomed(cx, "a");
            (host, guest)
        });
        assert_zoomed(&app, Some("a"));
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(gpui::px(800.), gpui::px(600.)));
        window
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a"]);
        assert_eq!(
            shown.borrow()[0].1,
            gpui::Bounds {
                origin: gpui::point(gpui::px(0.), gpui::px(0.)),
                size: gpui::size(gpui::px(800.), gpui::px(600.))
            }
        );
        let original: BTreeMap<_, _> = views
            .borrow()
            .iter()
            .map(|(node, view)| (node.clone(), view.upgrade().unwrap().entity_id()))
            .collect();
        assert_eq!(original.len(), 3, "all views are created even while zoomed");
        app.update(|cx| {
            views.borrow()["a"]
                .upgrade()
                .unwrap()
                .update(cx, |card, _| card.marker = 2)
        });
        app.update(|cx| guest.zoom_out(cx));
        assert_zoomed(&app, None);
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "b"]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let point = |x| gpui::point(gpui::px(x), gpui::px(100.));
        visual.simulate_mouse_move(point(400.), None, Default::default());
        visual.simulate_mouse_down(point(400.), gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(point(500.), gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(point(500.), gpui::MouseButton::Left, Default::default());
        draw(window, &mut app, &shown);
        let tab = visual.debug_bounds("TAB-c").unwrap();
        visual.simulate_click(tab.center(), Default::default());
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a", "c"]);
        let before = shown.borrow().clone();
        assert_eq!(before[0].1.size.width, gpui::px(500.));
        let selection_logs = trace
            .0
            .lock()
            .unwrap()
            .lines
            .iter()
            .filter(|line| line.contains("tab selected"))
            .count();
        app.update(|cx| guest.set_zoomed(cx, "c"));
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["c"]);
        assert_eq!(
            shown.borrow()[0].1.size,
            gpui::size(gpui::px(800.), gpui::px(600.))
        );
        assert!(
            visual.debug_bounds("TAB-c").is_none(),
            "only the zoomed view is drawn"
        );
        app.update(|cx| host.set_zoomed(cx, "a"));
        assert_zoomed(&app, Some("a"));
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["a"]);
        app.update(|cx| {
            for node in ["missing", "root", "Bad", "", "0bad", "bad.id"] {
                guest.set_zoomed(cx, node);
                assert_eq!(riso_shell::dev::zoomed(cx).as_deref(), Some("a"));
            }
            host.set_hidden(cx, "b", true);
            guest.set_zoomed(cx, "b");
            assert_eq!(riso_shell::dev::zoomed(cx).as_deref(), Some("a"));
            host.set_hidden(cx, "b", false);
            host.set_hidden(cx, "right", true);
            guest.set_zoomed(cx, "c");
            assert_eq!(riso_shell::dev::zoomed(cx).as_deref(), Some("a"));
            host.set_hidden(cx, "right", false);
            guest.zoom_out(cx);
        });
        draw(window, &mut app, &shown);
        assert_eq!(
            *shown.borrow(),
            before,
            "shares and selected c are exactly restored"
        );
        assert_eq!(
            trace
                .0
                .lock()
                .unwrap()
                .lines
                .iter()
                .filter(|line| line.contains("tab selected"))
                .count(),
            selection_logs
        );
        for (node, view) in views.borrow().iter() {
            assert_eq!(view.upgrade().unwrap().entity_id(), original[node]);
        }
        assert_eq!(
            app.update(|cx| views.borrow()["a"].upgrade().unwrap().read(cx).marker),
            2
        );
        // Zooming an unselected tab does not select it upon restoration.
        app.update(|cx| guest.set_zoomed(cx, "b"));
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["b"]);
        app.update(|cx| guest.zoom_out(cx));
        draw(window, &mut app, &shown);
        assert_eq!(*shown.borrow(), before);
        // Containers are valid zoom targets too.
        app.update(|cx| guest.set_zoomed(cx, "right"));
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["c"]);
        assert_eq!(shown.borrow()[0].1.size.width, gpui::px(800.));
        app.update(|cx| guest.zoom_out(cx));
        // Both direct and ancestor hide clear zoom, without automatically restoring it.
        for hidden in ["c", "right"] {
            app.update(|cx| {
                guest.set_zoomed(cx, "c");
                host.set_hidden(cx, hidden, true);
            });
            assert_zoomed(&app, None);
            draw(window, &mut app, &shown);
            assert!(!nodes(&shown).contains(&"c".to_owned()));
            app.update(|cx| host.set_hidden(cx, hidden, false));
            assert_zoomed(&app, None);
        }
        app.update(|cx| {
            host.set_hidden(cx, "right", true);
            guest.set_zoomed(cx, "a");
            host.move_node(cx, "a", "right", "children", 0);
        });
        assert_zoomed(&app, None);
        draw(window, &mut app, &shown);
        assert!(shown.borrow().is_empty());
        app.update(|cx| {
            host.move_node(cx, "a", "root", "children", 0);
            host.set_hidden(cx, "right", false);
        });
        // Refused operations and removing a sibling leave the zoom in place.
        app.update(|cx| {
            guest.set_zoomed(cx, "c");
            host.remove(cx, "root");
            host.set_hidden(cx, "root", true);
            host.remove(cx, "missing");
            host.remove(cx, "a");
        });
        assert_zoomed(&app, Some("c"));
        app.update(|cx| host.remove(cx, "c"));
        assert_zoomed(&app, None);
        draw(window, &mut app, &shown);
        assert_eq!(nodes(&shown), ["b"]);
        assert!(views.borrow()["c"].upgrade().is_none());
        app.update(|cx| {
            guest.set_zoomed(cx, "b");
            host.remove(cx, "right");
        });
        assert_zoomed(&app, None);
        draw(window, &mut app, &shown);
        assert!(shown.borrow().is_empty());
        assert!(views.borrow()["b"].upgrade().is_none());
        app.update(|cx| guest.zoom_out(cx));
        for (node, component, slot) in [
            ("no-component", "tour.missing", "children"),
            ("no-slot", "riso.row", "missing"),
        ] {
            let app = cx.new_app();
            app.update(riso_ui::init);
            let host = app.update(|cx| {
                let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
                host.place_root(cx, "root", "riso.row");
                host.place(cx, node, component, "root", slot);
                host.set_zoomed(cx, node);
                host
            });
            assert_zoomed(&app, Some(node));
            app.update(|cx| {
                cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                    .unwrap();
            });
            assert_zoomed(&app, None);
            app.update(|cx| host.set_zoomed(cx, node));
            assert_zoomed(&app, None);
        }
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    for line in [
        r#"zoom node="missing" extension="guest": INFO message=the node is not in the tree"#,
        r#"zoom node="root" extension="guest": INFO message=the root already fills the window"#,
        r#"zoom node="b" extension="guest": INFO message=the node is hidden or under a hidden node"#,
        r#"zoom node="c" extension="guest": INFO message=the node is hidden or under a hidden node"#,
        r#"zoom node="a" extension="tour": DEBUG message=replaced previous=c"#,
        r#"zoom node="c" extension="guest": DEBUG message=zoomed node="c""#,
        r#"zoom-out extension="guest": DEBUG message=zoomed out node=a"#,
        r#"zoom-out extension="guest": DEBUG message=nothing zoomed"#,
        r#"zoom node="Bad" extension="guest": ERROR message=invalid node ID "Bad": a slug must start with a lowercase ASCII letter"#,
        r#"layout-op op="move" node="a" extension="tour": DEBUG message=zoom cleared node=a"#,
        r#"shell-create root=root: DEBUG message=zoom cleared node=no-component"#,
        r#"shell-create root=root: DEBUG message=zoom cleared node=no-slot"#,
        r#"zoom node="no-component" extension="tour": INFO message=the node has no view"#,
        r#"zoom node="no-slot" extension="tour": INFO message=the node has no view"#,
        r#"layout-op op="hide" node="c" extension="tour": DEBUG message=zoom cleared node=c"#,
        r#"layout-op op="hide" node="right" extension="tour": DEBUG message=zoom cleared node=c"#,
        r#"layout-op op="remove" node="c" extension="tour": DEBUG message=zoom cleared node=c"#,
        r#"layout-op op="remove" node="right" extension="tour": DEBUG message=zoom cleared node=b"#,
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
            .filter(|line| line.contains("zoom cleared"))
            .count(),
        7
    );
    assert_eq!(
        captured
            .lines
            .iter()
            .filter(|line| line.contains(": ERROR"))
            .count(),
        6
    );
}
