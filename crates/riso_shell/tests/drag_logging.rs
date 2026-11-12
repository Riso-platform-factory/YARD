#![cfg(feature = "dev")]

use gpui::{AppContext, Bounds, IntoElement, Pixels, Point, Styled, TestAppContext, px};
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
    trace_edges: usize,
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
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            && !event.metadata().target().starts_with("riso_layout")
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        if *event.metadata().level() == tracing::Level::TRACE {
            if fields.0.contains("drag edge") {
                captured.trace_edges += 1;
            }
            return;
        }
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

type Shown = Rc<RefCell<Vec<(String, Bounds<Pixels>)>>>;

struct Card {
    node: String,
    marker: usize,
    shown: Shown,
    slots: riso_shell::dev::Slots,
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
            .children(
                self.slots
                    .views("body")
                    .into_iter()
                    .map(|view| gpui::div().flex_1().min_h_0().child(view)),
            )
    }
}

fn draw(
    window: gpui::WindowHandle<riso_shell::dev::Shell>,
    cx: &mut TestAppContext,
    shown: &Shown,
) {
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

fn card_bounds(shown: &Shown, name: &str) -> Bounds<Pixels> {
    shown
        .borrow()
        .iter()
        .find(|(node, _)| node == name)
        .unwrap_or_else(|| panic!("missing card {name} in {:?}", shown.borrow()))
        .1
}

fn assert_no_overlays(visual: &mut gpui::VisualTestContext) {
    for selector in [
        "DROP-OVERLAY-left",
        "DROP-OVERLAY-right",
        "DROP-OVERLAY-top",
        "DROP-OVERLAY-bottom",
    ] {
        assert!(
            visual.debug_bounds(selector).is_none(),
            "unexpected {selector}"
        );
    }
}

fn drag_tab(
    window: gpui::WindowHandle<riso_shell::dev::Shell>,
    app: &mut TestAppContext,
    visual: &mut gpui::VisualTestContext,
    shown: &Shown,
    from_selector: &'static str,
    to: Point<Pixels>,
    expected_overlay: Option<(&'static str, Bounds<Pixels>)>,
) {
    let start = visual
        .debug_bounds(from_selector)
        .unwrap_or_else(|| panic!("missing {from_selector}"))
        .center();
    visual.simulate_mouse_move(start, None, Default::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    let nudge = gpui::point(start.x + px(5.), start.y);
    visual.simulate_mouse_move(nudge, gpui::MouseButton::Left, Default::default());
    draw(window, app, shown);

    let mid = gpui::point((start.x + to.x) * 0.5, (start.y + to.y) * 0.5);
    visual.simulate_mouse_move(mid, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
    draw(window, app, shown);

    match expected_overlay {
        Some((selector, expected)) => {
            let actual = visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing overlay {selector}"));
            assert_eq!(actual, expected, "overlay bounds for {selector}");
        }
        None => assert_no_overlays(visual),
    }

    visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
    draw(window, app, shown);
    assert_no_overlays(visual);
}

// Alone in this binary: all captured log lines run under one subscriber.
#[gpui::test]
fn drags_move_tabs_onto_bars_and_edges_and_log_as_riso(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        tab_bars_row_edges_own_position_and_ignored_drops(cx, &trace);
        column_edges_and_cross_axis_drop(cx, &trace);
        cycle_drop_is_refused(cx, &trace);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    assert!(
        captured.trace_edges > 0,
        "edge computation must log at trace level"
    );
    assert!(
        !captured.lines.iter().any(|line| line.contains("drag edge")),
        "edge computation must never log above trace"
    );
}

fn tab_bars_row_edges_own_position_and_ignored_drops(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();
    let entities = Rc::new(RefCell::new(BTreeMap::new()));

    app.update(|cx| {
        let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
        let shown = shown.clone();
        let entities = entities.clone();
        host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            let marker = if node.as_str() == "a" { 42 } else { 0 };
            let card = cx.new(|_| Card {
                node: node.to_string(),
                marker,
                shown: shown.clone(),
                slots,
            });
            entities.borrow_mut().insert(node.to_string(), card.clone());
            card.into()
        });
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "left", "riso.tabs", "root", "children");
        host.place(cx, "right", "riso.tabs", "root", "children");
        host.place(cx, "a", "tour.card", "left", "children");
        host.place(cx, "b", "tour.card", "left", "children");
        host.place(cx, "c", "tour.card", "right", "children");
        host.place(cx, "d", "tour.card", "right", "children");
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(1200.), px(600.)));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "c"]);

    let a_id = entities.borrow()["a"].entity_id();
    let c_id = entities.borrow()["c"].entity_id();

    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

    // 1. Dropping a tab on its own current position (onto itself, and last tab onto its own bar)
    // logs `drop ignored` without calling move.
    let before_own = trace.0.lock().unwrap().lines.len();
    let tab_a_center = visual.debug_bounds("TAB-a").unwrap().center();
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        tab_a_center,
        None,
    );
    let left_bar_end = {
        let bar = visual.debug_bounds("TABBAR-left").unwrap();
        gpui::point(bar.right() - px(10.), bar.center().y)
    };
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        left_bar_end,
        None,
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_own..],
            [
                "none: DEBUG message=drag started node=a",
                "none: DEBUG message=drop ignored node=a",
                "none: DEBUG message=drag started node=b",
                "none: DEBUG message=drop ignored node=b",
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["a", "c"]);

    // 2. Drop `a` onto the empty part of `right`'s tab bar (near the right edge of `right` so we
    // also confirm `root`'s split-edge overlay does not claim the tab bar).
    let before_bar_end = trace.0.lock().unwrap().lines.len();
    let right_bar_end = {
        let bar = visual.debug_bounds("TABBAR-right").unwrap();
        gpui::point(bar.right() - px(10.), bar.center().y)
    };
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        right_bar_end,
        None,
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_bar_end..],
            [
                "none: DEBUG message=drag started node=a",
                r#"layout-op op="move" node="a" extension="riso": DEBUG message=node moved node=a parent=right slot=children index=2"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["b", "c"]);
    assert!(
        visual.debug_bounds("TAB-d").unwrap().origin.x
            < visual.debug_bounds("TAB-a").unwrap().origin.x
    );

    // Clicking `TAB-a` in `right` reveals the exact same entity and state.
    let tab_a = visual.debug_bounds("TAB-a").unwrap().center();
    visual.simulate_click(tab_a, Default::default());
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["b", "a"]);
    assert_eq!(entities.borrow()["a"].entity_id(), a_id);
    assert_eq!(app.update(|cx| entities.borrow()["a"].read(cx).marker), 42);

    // 3. Drop `a` onto tab `c` (index 0 in `right`), and `b` onto tab `c` (index 1 in `right`).
    let before_tab_idx = trace.0.lock().unwrap().lines.len();
    let tab_c = visual.debug_bounds("TAB-c").unwrap().center();
    drag_tab(window, &mut app, &mut visual, &shown, "TAB-a", tab_c, None);
    assert!(
        visual.debug_bounds("TAB-a").unwrap().origin.x
            < visual.debug_bounds("TAB-c").unwrap().origin.x
    );
    let tab_c = visual.debug_bounds("TAB-c").unwrap().center();
    drag_tab(window, &mut app, &mut visual, &shown, "TAB-b", tab_c, None);
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_tab_idx..],
            [
                "none: DEBUG message=drag started node=a",
                r#"layout-op op="move" node="a" extension="riso": DEBUG message=node moved node=a parent=right slot=children index=0"#,
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="move" node="b" extension="riso": DEBUG message=node moved node=b parent=right slot=children index=1"#,
            ]
        );
    }
    assert!(
        visual.debug_bounds("TAB-a").unwrap().origin.x
            < visual.debug_bounds("TAB-b").unwrap().origin.x
    );
    assert!(
        visual.debug_bounds("TAB-b").unwrap().origin.x
            < visual.debug_bounds("TAB-c").unwrap().origin.x
    );

    // Put `d` back into `left`'s empty tab bar so both halves have cards.
    let left_bar = visual.debug_bounds("TABBAR-left").unwrap().center();
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-d",
        left_bar,
        None,
    );
    assert_eq!(shown_nodes(&shown), ["d", "a"]);

    // 4. Hover `c` over `left`'s right edge, leave into `left`'s middle (clearing the overlay),
    // re-enter `left`'s right edge, and drop (`Edge::After` on `left`, index 1 in `root`).
    let before_after_edge = trace.0.lock().unwrap().lines.len();
    let start_c = visual.debug_bounds("TAB-c").unwrap().center();
    visual.simulate_mouse_move(start_c, None, Default::default());
    visual.simulate_mouse_down(start_c, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(start_c.x + px(5.), start_c.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    visual.simulate_mouse_move(
        gpui::point(px(580.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    assert_eq!(
        visual.debug_bounds("DROP-OVERLAY-right"),
        Some(Bounds {
            origin: gpui::point(px(300.), px(0.)),
            size: gpui::size(px(300.), px(600.)),
        })
    );
    // Leave the edge zone into the middle of `left`: overlay clears while drag continues.
    visual.simulate_mouse_move(
        gpui::point(px(300.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    assert_no_overlays(&mut visual);
    // Re-enter `left`'s right edge and drop.
    visual.simulate_mouse_move(
        gpui::point(px(580.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    assert!(visual.debug_bounds("DROP-OVERLAY-right").is_some());
    visual.simulate_mouse_up(
        gpui::point(px(580.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    assert_no_overlays(&mut visual);
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_after_edge..],
            [
                "none: DEBUG message=drag started node=c",
                r#"layout-op op="move" node="c" extension="riso": DEBUG message=node moved node=c parent=root slot=children index=1"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["d", "c", "a"]);
    assert_eq!(entities.borrow()["c"].entity_id(), c_id);
    assert_eq!(card_bounds(&shown, "c").size.width, px(400.));

    // 5. Drop `b` onto `left`'s left edge (`Edge::Before` on `left`, index 0 in `root`).
    // `left` currently occupies `x: 0..400, y: 0..600`, so its left half is `0..200 × 0..600`.
    let before_left_edge = trace.0.lock().unwrap().lines.len();
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        gpui::point(px(20.), px(300.)),
        Some((
            "DROP-OVERLAY-left",
            Bounds {
                origin: gpui::point(px(0.), px(0.)),
                size: gpui::size(px(200.), px(600.)),
            },
        )),
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_left_edge..],
            [
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="move" node="b" extension="riso": DEBUG message=node moved node=b parent=root slot=children index=0"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["b", "d", "c", "a"]);
    assert_eq!(card_bounds(&shown, "b").size.width, px(300.));

    // 6. Ignored drops: middle of card `c`, and top edge of card `c` (across `riso.row`'s axis).
    let before_ignored = trace.0.lock().unwrap().lines.len();
    let c_rect = card_bounds(&shown, "c");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        c_rect.center(),
        None,
    );
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(c_rect.center().x, c_rect.origin.y + px(10.)),
        None,
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_ignored..],
            [
                "none: DEBUG message=drag started node=a",
                "none: DEBUG message=drop ignored node=a",
                "none: DEBUG message=drag started node=a",
                "none: DEBUG message=drop ignored node=a",
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["b", "d", "c", "a"]);
}

fn column_edges_and_cross_axis_drop(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();

    let host = app.update(|cx| {
        let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
        let shown = shown.clone();
        host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                marker: 0,
                shown: shown.clone(),
                slots,
            })
            .into()
        });
        host.place_root(cx, "root", "riso.column");
        host.place(cx, "tabs", "riso.tabs", "root", "children");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "tabs", "children");
        }
        host.place(cx, "hidden", "tour.card", "root", "children");
        host.place(cx, "d", "tour.card", "root", "children");
        host.set_hidden(cx, "hidden", true);
        host
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(600.), px(600.)));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "d"]);

    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

    // Drop `b` onto `d`'s top edge (`Edge::Before` on shown index 1 `d`, slot index 2).
    let before = trace.0.lock().unwrap().lines.len();
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        gpui::point(px(300.), px(315.)),
        Some((
            "DROP-OVERLAY-top",
            Bounds {
                origin: gpui::point(px(0.), px(300.)),
                size: gpui::size(px(600.), px(150.)),
            },
        )),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "d"]);
    assert_eq!(card_bounds(&shown, "b").size.height, px(200.));

    // Drop `c` onto `d`'s bottom edge (`Edge::After` on shown index 2 `d`, slot index 4).
    // `d` now spans `y: 400..600`, so its bottom half is `y: 500..600`.
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-c",
        gpui::point(px(300.), px(585.)),
        Some((
            "DROP-OVERLAY-bottom",
            Bounds {
                origin: gpui::point(px(0.), px(500.)),
                size: gpui::size(px(600.), px(100.)),
            },
        )),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "d", "c"]);
    assert_eq!(card_bounds(&shown, "c").size.height, px(150.));

    // Dropping `a` onto `b`'s left edge (across `riso.column`'s vertical axis) is ignored.
    let b_rect = card_bounds(&shown, "b");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(px(10.), b_rect.center().y),
        None,
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before..],
            [
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="move" node="b" extension="riso": DEBUG message=node moved node=b parent=root slot=children index=2"#,
                "none: DEBUG message=drag started node=c",
                r#"layout-op op="move" node="c" extension="riso": DEBUG message=node moved node=c parent=root slot=children index=4"#,
                "none: DEBUG message=drag started node=a",
                "none: DEBUG message=drop ignored node=a",
            ]
        );
    }

    // Showing `hidden` confirms `b` landed between `hidden` and `d`, and `c` landed after `d`.
    app.update(|cx| host.set_hidden(cx, "hidden", false));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "hidden", "b", "d", "c"]);
}

fn cycle_drop_is_refused(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(riso_ui::init);
    let shown = Shown::default();

    app.update(|cx| {
        let host = riso_shell::dev::Extension::new(cx, "tour").unwrap();
        let shown = shown.clone();
        host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                marker: 0,
                shown: shown.clone(),
                slots,
            })
            .into()
        });
        host.place_root(cx, "root", "riso.tabs");
        host.place(cx, "branch", "tour.card", "root", "children");
        host.place(cx, "other", "tour.card", "root", "children");
        host.place(cx, "inner", "riso.row", "branch", "body");
        host.place(cx, "leaf", "tour.card", "inner", "children");
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(600.), px(600.)));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["branch", "leaf"]);

    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let leaf_rect = card_bounds(&shown, "leaf");
    let before = trace.0.lock().unwrap().lines.len();
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-branch",
        gpui::point(leaf_rect.origin.x + px(15.), leaf_rect.center().y),
        Some((
            "DROP-OVERLAY-left",
            Bounds {
                origin: leaf_rect.origin,
                size: gpui::size(leaf_rect.size.width * 0.5, leaf_rect.size.height),
            },
        )),
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before..],
            [
                "none: DEBUG message=drag started node=branch",
                r#"layout-op op="move" node="branch" extension="riso": INFO message=moving a node into itself or its subtree would make a cycle node=branch parent=inner slot=children index=0"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["branch", "leaf"]);
}
