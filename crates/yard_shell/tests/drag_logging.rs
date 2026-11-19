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
    trace_zones: usize,
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
        if !event.metadata().target().starts_with("yard_shell")
            && !event.metadata().target().starts_with("yard_layout")
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        if *event.metadata().level() == tracing::Level::TRACE {
            if fields.0.contains("drop zone") {
                captured.trace_zones += 1;
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
    slots: yard_shell::Slots,
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

fn draw(window: gpui::WindowHandle<yard_shell::Shell>, cx: &mut TestAppContext, shown: &Shown) {
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
        "DROP-OVERLAY-center",
    ] {
        assert!(
            visual.debug_bounds(selector).is_none(),
            "unexpected {selector}"
        );
    }
}

fn drag_tab(
    window: gpui::WindowHandle<yard_shell::Shell>,
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
fn drags_move_tabs_onto_bars_and_edges_and_log_as_yard(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        tab_bars_row_edges_own_position_and_ignored_drops(cx, &trace);
        column_edges_and_cross_axis_drop(cx, &trace);
        cycle_drop_is_refused(cx, &trace);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    assert!(
        captured.trace_zones > 0,
        "zone computation must log at trace level"
    );
    assert!(
        !captured.lines.iter().any(|line| line.contains("drop zone")),
        "zone computation must never log above trace"
    );
}

fn tab_bars_row_edges_own_position_and_ignored_drops(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(yard_ui::init);
    let shown = Shown::default();
    let entities = Rc::new(RefCell::new(BTreeMap::new()));

    app.update(|cx| {
        let host = yard_shell::Extension::new(cx, "tour").unwrap();
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
        host.place_root(cx, "root", "yard.row");
        host.place(cx, "left", "yard.tabs", "root", "children");
        host.place(cx, "right", "yard.tabs", "root", "children");
        host.place(cx, "a", "tour.card", "left", "children");
        host.place(cx, "b", "tour.card", "left", "children");
        host.place(cx, "c", "tour.card", "right", "children");
        host.place(cx, "d", "tour.card", "right", "children");
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
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
                "focus-change node=left: DEBUG message=focused",
                "none: DEBUG message=drag started node=a",
                "window id=4294967297: DEBUG message=drop ignored node=a",
                "none: DEBUG message=drag started node=b",
                "window id=4294967297: DEBUG message=drop ignored node=b",
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
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=node moved node=a parent=right slot=children index=2"#,
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=recorded user_layer=1"#,
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
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=node moved node=a parent=right slot=children index=0"#,
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=recorded user_layer=2"#,
                "focus-change node=left: DEBUG message=focused",
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=node moved node=b parent=right slot=children index=1"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=recorded user_layer=3"#,
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

    // 4. Hover `c` over `d`'s right edge, leave into `d`'s middle (the center
    // zone highlights the whole card), re-enter `d`'s right edge, and drop:
    // `d` is shown in `left`, a tabs container, so any of its edges splits a
    // `yard.row` around `left` with `c` in a new `yard.tabs` beside it.
    let before_after_edge = trace.0.lock().unwrap().lines.len();
    let d_rect = card_bounds(&shown, "d");
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
            origin: gpui::point(d_rect.origin.x + d_rect.size.width * 0.5, d_rect.origin.y,),
            size: gpui::size(d_rect.size.width * 0.5, d_rect.size.height),
        })
    );
    // The middle of `d` is a center drop zone: the whole card is highlighted.
    visual.simulate_mouse_move(
        gpui::point(px(300.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    assert_eq!(visual.debug_bounds("DROP-OVERLAY-center"), Some(d_rect));
    // Re-enter `d`'s right edge and drop.
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
                r#"layout-op op="add" node="split-1" extension="yard": DEBUG message=node added node=split-1 component=yard.row extension=yard parent=root slot=children index=0"#,
                r#"layout-op op="add" node="split-1" extension="yard": DEBUG message=recorded user_layer=5"#,
                r#"props-read node="split-1" component="yard.row": DEBUG message=props read keys="shares""#,
                r#"component-create node=split-1 component=yard.row extension=yard: DEBUG message=split created axis="row" children=0"#,
                r#"layout-op op="move" node="left" extension="yard": DEBUG message=node moved node=left parent=split-1 slot=children index=0"#,
                r#"layout-op op="move" node="left" extension="yard": DEBUG message=recorded user_layer=6"#,
                r#"layout-op op="add" node="tabs-1" extension="yard": DEBUG message=node added node=tabs-1 component=yard.tabs extension=yard parent=split-1 slot=children"#,
                r#"layout-op op="add" node="tabs-1" extension="yard": DEBUG message=recorded user_layer=7"#,
                r#"component-create node=tabs-1 component=yard.tabs extension=yard: DEBUG message=tabs created tabs=0"#,
                r#"layout-op op="move" node="c" extension="yard": DEBUG message=node moved node=c parent=tabs-1 slot=children index=0"#,
                r#"layout-op op="move" node="c" extension="yard": DEBUG message=recorded user_layer=8"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["d", "c", "a"]);
    assert_eq!(entities.borrow()["c"].entity_id(), c_id);
    assert_eq!(card_bounds(&shown, "c").size.width, px(300.));

    // 5. Drop `b` onto `d`'s left edge: another split around `left`, with `b`'s
    // new tabs container before it (`left` is 300px wide inside `split-1`).
    let before_left_edge = trace.0.lock().unwrap().lines.len();
    let d_rect = card_bounds(&shown, "d");
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
                origin: d_rect.origin,
                size: gpui::size(d_rect.size.width * 0.5, d_rect.size.height),
            },
        )),
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_left_edge..],
            [
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="add" node="split-2" extension="yard": DEBUG message=node added node=split-2 component=yard.row extension=yard parent=split-1 slot=children index=0"#,
                r#"layout-op op="add" node="split-2" extension="yard": DEBUG message=recorded user_layer=9"#,
                r#"props-read node="split-2" component="yard.row": DEBUG message=props read keys="shares""#,
                r#"component-create node=split-2 component=yard.row extension=yard: DEBUG message=split created axis="row" children=0"#,
                r#"layout-op op="move" node="left" extension="yard": DEBUG message=node moved node=left parent=split-2 slot=children index=0"#,
                r#"layout-op op="move" node="left" extension="yard": DEBUG message=recorded user_layer=10"#,
                r#"layout-op op="add" node="tabs-2" extension="yard": DEBUG message=node added node=tabs-2 component=yard.tabs extension=yard parent=split-2 slot=children index=0"#,
                r#"layout-op op="add" node="tabs-2" extension="yard": DEBUG message=recorded user_layer=11"#,
                r#"component-create node=tabs-2 component=yard.tabs extension=yard: DEBUG message=tabs created tabs=0"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=node moved node=b parent=tabs-2 slot=children index=0"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=recorded user_layer=12"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["b", "d", "c", "a"]);
    assert_eq!(card_bounds(&shown, "b").size.width, px(150.));

    // 6. The center of a tabs container's content moves the node into it and
    // selects it; a drop on the edge of the node's own content is refused as
    // a cycle, and the operations that ran are undone.
    let before_split = trace.0.lock().unwrap().lines.len();
    let c_rect = card_bounds(&shown, "c");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        c_rect.center(),
        Some(("DROP-OVERLAY-center", c_rect)),
    );
    // `a` is selected in `tabs-1` and shown where `c` was: the top edge is
    // `a`'s own, so the split around `tabs-1` is refused at its fourth
    // operation, moving `a` beside itself.
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(c_rect.center().x, c_rect.origin.y + px(10.)),
        Some((
            "DROP-OVERLAY-top",
            Bounds {
                origin: c_rect.origin,
                size: gpui::size(c_rect.size.width, c_rect.size.height * 0.5),
            },
        )),
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before_split..],
            [
                "none: DEBUG message=drag started node=a",
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=node moved node=a parent=tabs-1 slot=children index=1"#,
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=recorded user_layer=13"#,
                "window id=4294967297: DEBUG message=tab selected tabs=tabs-1 tab=a",
                "focus-change node=tabs-1: DEBUG message=focused",
                "none: DEBUG message=drag started node=a",
                r#"layout-op op="add" node="split-3" extension="yard": DEBUG message=node added node=split-3 component=yard.column extension=yard parent=split-1 slot=children index=1"#,
                r#"layout-op op="add" node="split-3" extension="yard": DEBUG message=recorded user_layer=14"#,
                r#"props-read node="split-3" component="yard.column": DEBUG message=props read keys="shares""#,
                r#"component-create node=split-3 component=yard.column extension=yard: DEBUG message=split created axis="column" children=0"#,
                r#"layout-op op="move" node="tabs-1" extension="yard": DEBUG message=node moved node=tabs-1 parent=split-3 slot=children index=0"#,
                r#"layout-op op="move" node="tabs-1" extension="yard": DEBUG message=recorded user_layer=15"#,
                r#"layout-op op="add" node="tabs-3" extension="yard": DEBUG message=node added node=tabs-3 component=yard.tabs extension=yard parent=split-3 slot=children index=0"#,
                r#"layout-op op="add" node="tabs-3" extension="yard": DEBUG message=recorded user_layer=16"#,
                r#"component-create node=tabs-3 component=yard.tabs extension=yard: DEBUG message=tabs created tabs=0"#,
                r#"layout-op op="move" node="a" extension="yard": INFO message=moving a node onto itself would make a cycle"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=user layer dropped dropped=16"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=a parent=right slot=children index=2"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=1"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=a parent=right slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=2"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=b parent=right slot=children index=1"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=3"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=d parent=left slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=4"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node added node=split-1 component=yard.row extension=yard parent=root slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=5"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=left parent=split-1 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=6"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node added node=tabs-1 component=yard.tabs extension=yard parent=split-1 slot=children"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=7"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=c parent=tabs-1 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=8"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node added node=split-2 component=yard.row extension=yard parent=split-1 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=9"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=left parent=split-2 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=10"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node added node=tabs-2 component=yard.tabs extension=yard parent=split-2 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=11"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=b parent=tabs-2 slot=children index=0"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=12"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=node moved node=a parent=tabs-1 slot=children index=1"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=recorded user_layer=13"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=view dropped node=tabs-3"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=view dropped node=split-3"#,
                r#"split-on-drop node=a target=tabs-1 edge="top": DEBUG message=split on drop undone done=3"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["b", "d", "a"]);
}

fn column_edges_and_cross_axis_drop(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(yard_ui::init);
    let shown = Shown::default();

    let host = app.update(|cx| {
        let host = yard_shell::Extension::new(cx, "tour").unwrap();
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
        host.place_root(cx, "root", "yard.column");
        host.place(cx, "tabs", "yard.tabs", "root", "children");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "tabs", "children");
        }
        host.place(cx, "hidden", "tour.card", "root", "children");
        host.place(cx, "d", "tour.card", "root", "children");
        host.set_hidden(cx, "hidden", true);
        host
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(600.), px(600.)));
    draw(window, &mut app, &shown);
    let body = app.update(|cx| yard_shell::node_bounds(cx, "body").unwrap());
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
        gpui::point(px(300.), body.top() + body.size.height / 2. + px(15.)),
        Some((
            "DROP-OVERLAY-top",
            Bounds {
                origin: gpui::point(px(0.), body.top() + body.size.height / 2.),
                size: gpui::size(px(600.), body.size.height / 4.),
            },
        )),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "d"]);
    assert_eq!(
        card_bounds(&shown, "b").size.height,
        (body.size.height / 3.).round()
    );

    // Drop `c` onto `d`'s bottom edge (`Edge::After` on shown index 2 `d`, slot index 4).
    // `d` now fills the last third of the body; the overlay fills its bottom half.
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
                origin: gpui::point(px(0.), body.top() + (body.size.height / 3.).round() * 2.5),
                size: gpui::size(px(600.), (body.size.height / 3.).round() / 2.),
            },
        )),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "d", "c"]);
    assert_eq!(
        card_bounds(&shown, "c").size.height,
        (body.size.height / 4.).round()
    );

    // Dropping `a` onto `b`'s left edge (across `yard.column`'s vertical axis)
    // splits a `yard.row` around `b`, with `a` in a new `yard.tabs` before it.
    let b_rect = card_bounds(&shown, "b");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(px(10.), b_rect.center().y),
        Some((
            "DROP-OVERLAY-left",
            Bounds {
                origin: b_rect.origin,
                size: gpui::size(b_rect.size.width * 0.5, b_rect.size.height),
            },
        )),
    );
    {
        let captured = trace.0.lock().unwrap();
        assert_eq!(
            &captured.lines[before..],
            [
                "focus-change node=tabs: DEBUG message=focused",
                "none: DEBUG message=drag started node=b",
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=node moved node=b parent=root slot=children index=2"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=recorded user_layer=1"#,
                "none: DEBUG message=drag started node=c",
                r#"layout-op op="move" node="c" extension="yard": DEBUG message=node moved node=c parent=root slot=children index=4"#,
                r#"layout-op op="move" node="c" extension="yard": DEBUG message=recorded user_layer=2"#,
                "none: DEBUG message=drag started node=a",
                r#"layout-op op="add" node="split-1" extension="yard": DEBUG message=node added node=split-1 component=yard.row extension=yard parent=root slot=children index=2"#,
                r#"layout-op op="add" node="split-1" extension="yard": DEBUG message=recorded user_layer=3"#,
                r#"props-read node="split-1" component="yard.row": DEBUG message=props read keys="shares""#,
                r#"component-create node=split-1 component=yard.row extension=yard: DEBUG message=split created axis="row" children=0"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=node moved node=b parent=split-1 slot=children index=0"#,
                r#"layout-op op="move" node="b" extension="yard": DEBUG message=recorded user_layer=4"#,
                r#"layout-op op="add" node="tabs-1" extension="yard": DEBUG message=node added node=tabs-1 component=yard.tabs extension=yard parent=split-1 slot=children index=0"#,
                r#"layout-op op="add" node="tabs-1" extension="yard": DEBUG message=recorded user_layer=5"#,
                r#"component-create node=tabs-1 component=yard.tabs extension=yard: DEBUG message=tabs created tabs=0"#,
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=node moved node=a parent=tabs-1 slot=children index=0"#,
                r#"layout-op op="move" node="a" extension="yard": DEBUG message=recorded user_layer=6"#,
            ]
        );
    }

    // Showing `hidden` confirms `b`'s split landed between `hidden` and `d`,
    // and `c` landed after `d`.
    app.update(|cx| host.set_hidden(cx, "hidden", false));
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["hidden", "a", "b", "d", "c"]);
}

fn cycle_drop_is_refused(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    app.update(yard_ui::init);
    let shown = Shown::default();

    app.update(|cx| {
        let host = yard_shell::Extension::new(cx, "tour").unwrap();
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
        host.place_root(cx, "root", "yard.tabs");
        host.place(cx, "branch", "tour.card", "root", "children");
        host.place(cx, "other", "tour.card", "root", "children");
        host.place(cx, "inner", "yard.row", "branch", "body");
        host.place(cx, "leaf", "tour.card", "inner", "children");
    });

    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
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
                "focus-change node=root: DEBUG message=focused",
                "none: DEBUG message=drag started node=branch",
                r#"layout-op op="move" node="branch" extension="yard": INFO message=moving a node into itself or its subtree would make a cycle node=branch parent=inner slot=children index=0"#,
            ]
        );
    }
    assert_eq!(shown_nodes(&shown), ["branch", "leaf"]);
}
