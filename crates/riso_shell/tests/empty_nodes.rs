#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_layout::NodeId;
use riso_shell::dev as shell;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::{Mutex, Once},
};

// Every test in this file shares one process-wide logger and runs under one
// lock, so a test never sees another test's diagnostic records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }
    fn flush(&self) {}
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let guard = LOCK.lock().unwrap();
    RECORDS.lock().unwrap().clear();
    guard
}

fn errors() -> Vec<String> {
    RECORDS
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message.clone())
        .collect()
}

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

// A card fills the zone its container gives it and is found as `CARD-<node>`.
// Its `empty` flag is the state its view reports through `Emptiable`.
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

// The view each node got, so a test can change its state and notify.
type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

// The nodes whose cards say they are empty from the start.
type Starts = Rc<RefCell<BTreeSet<String>>>;

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
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
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

// `host` registers the components of these tests: `tour.card`, which says it
// can be empty, and `tour.plain`, which does not, so its views never are.
fn setup(app: &mut TestAppContext) -> (shell::Extension, shell::Extension, Views, Starts) {
    let views = Views::default();
    let starts = Starts::default();
    let (host, guest) = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let guest = shell::Extension::new(cx, "guest").unwrap();
        for component in ["tour.card", "tour.plain"] {
            let views = views.clone();
            let starts = starts.clone();
            host.register(cx, component, &[], move |node, _, _, cx| {
                let card = cx.new(|_| Card {
                    node: node.to_string(),
                    empty: starts.borrow().contains(node.as_str()),
                });
                views
                    .borrow_mut()
                    .insert(node.to_string(), card.downgrade());
                card.into()
            });
        }
        host.register_empty::<Card>(cx, "tour.card");
        guest.register(cx, "guest.card", &[], move |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                empty: false,
            })
            .into()
        });
        (host, guest)
    });
    (host, guest, views, starts)
}

// The view changes its state and notifies; the next frame lays the window out
// again.
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

// The drawn bounds of a card's `CARD-<node>` element; an empty child has no
// room, so nothing draws at its place.
fn card(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    visual.debug_bounds(selector)
}

fn drawn_height(visual: &mut gpui::VisualTestContext, selector: &'static str) -> gpui::Pixels {
    card(visual, selector).map_or(px(0.), |bounds| bounds.size.height)
}

fn drawn_width(visual: &mut gpui::VisualTestContext, selector: &'static str) -> gpui::Pixels {
    card(visual, selector).map_or(px(0.), |bounds| bounds.size.width)
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
}

// The node the dev shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| shell::focused_node(window, cx).map(|node| node.to_string()))
}

// In a column of two cards, the empty one gets no room: the other takes the
// whole height. When the first stops being empty, both share the column again
// in their old proportions — its entry in `shares` is kept.
#[gpui::test]
fn an_empty_child_gets_no_room_and_shares_the_height_again(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, starts) = setup(&mut app);
    starts.borrow_mut().insert("top".to_string());
    app.update(|cx| {
        host.place_root_with_props(
            cx,
            "root",
            "riso.column",
            json!({"shares": {"top": 1, "bottom": 3}}),
        );
        host.place(cx, "top", "tour.card", "root", "children");
        host.place(cx, "bottom", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let body = app.update(|cx| shell::node_bounds(cx, "body")).unwrap();
    // `top` is empty from the start: it gets no room, and `bottom` has the
    // column's whole height.
    assert!(app.update(|cx| shell::is_empty(cx, "top")));
    assert!(!app.update(|cx| shell::is_empty(cx, "bottom")));
    assert!(!app.update(|cx| shell::is_empty(cx, "root")));
    assert_eq!(
        card(&mut visual, "CARD-bottom"),
        Some(body),
        "bottom takes the whole column"
    );
    assert_eq!(
        drawn_height(&mut visual, "CARD-top"),
        px(0.),
        "top gets no room"
    );
    // `top` stops being empty: the column is shared again in the old
    // proportions, one share against three.
    set_empty(&mut app, &views, "top", false);
    draw(window, &mut app);
    let top = card(&mut visual, "CARD-top").expect("top draws again");
    let bottom = card(&mut visual, "CARD-bottom").expect("bottom draws");
    let height = f32::from(body.size.height);
    assert!(
        (f32::from(top.size.height) - height / 4.).abs() < 1.,
        "top is a quarter of the column: {top:?} in {body:?}"
    );
    assert!(
        (f32::from(bottom.size.height) - height * 3. / 4.).abs() < 1.,
        "bottom is three quarters: {bottom:?}"
    );
    assert_eq!(top.bottom(), bottom.top(), "top sits above bottom");
    assert!(!app.update(|cx| shell::is_empty(cx, "top")));
    // Emptied again it leaves the whole height once more, and a second
    // return gives the same proportions back.
    set_empty(&mut app, &views, "top", true);
    draw(window, &mut app);
    assert_eq!(card(&mut visual, "CARD-bottom"), Some(body));
    assert_eq!(drawn_height(&mut visual, "CARD-top"), px(0.));
    set_empty(&mut app, &views, "top", false);
    draw(window, &mut app);
    let top = card(&mut visual, "CARD-top").expect("top draws again");
    assert!(
        (f32::from(top.size.height) - height / 4.).abs() < 1.,
        "top's share is kept: {top:?}"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A split whose children are all empty is itself empty, and its own parent
// gives it no room.
#[gpui::test]
fn a_split_whose_children_are_all_empty_is_itself_empty(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "left", "riso.column", "root", "children");
        host.place(cx, "l1", "tour.card", "left", "children");
        host.place(cx, "l2", "tour.card", "left", "children");
        host.place(cx, "right", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let body = app.update(|cx| shell::node_bounds(cx, "body")).unwrap();
    assert!(!app.update(|cx| shell::is_empty(cx, "left")));
    // One empty child is not enough: `l2` still shows, so `left` is not
    // empty and `l2` takes its whole height.
    set_empty(&mut app, &views, "l1", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_empty(cx, "l1")));
    assert!(!app.update(|cx| shell::is_empty(cx, "left")));
    assert_eq!(drawn_height(&mut visual, "CARD-l1"), px(0.));
    assert_eq!(
        card(&mut visual, "CARD-l2").expect("l2 draws").size.height,
        body.size.height,
        "l2 takes the column's whole height"
    );
    // All of `left`'s children empty: `left` is itself empty, and `root`
    // gives it no room — `right` takes the row.
    set_empty(&mut app, &views, "l2", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_empty(cx, "l2")));
    assert!(app.update(|cx| shell::is_empty(cx, "left")));
    assert!(!app.update(|cx| shell::is_hidden(cx, "left")));
    assert!(!app.update(|cx| shell::is_empty(cx, "root")));
    assert_eq!(
        card(&mut visual, "CARD-right"),
        Some(body),
        "right takes the whole row"
    );
    assert_eq!(drawn_width(&mut visual, "CARD-l2"), px(0.));
    // A child back: `left` is not empty and has its room again.
    set_empty(&mut app, &views, "l1", false);
    draw(window, &mut app);
    assert!(!app.update(|cx| shell::is_empty(cx, "left")));
    let right = card(&mut visual, "CARD-right").expect("right draws");
    assert!(
        (f32::from(right.size.width) - f32::from(body.size.width) / 2.).abs() < 1.,
        "left and right share the row again: {right:?} in {body:?}"
    );
    assert!(card(&mut visual, "CARD-l1").is_some());
    assert!(errors().is_empty(), "{:?}", errors());
}

// A tabs container drops an empty child's tab, and selects its first shown
// child when the selected one becomes empty.
#[gpui::test]
fn a_tabs_container_drops_an_empty_childs_tab_and_selects_the_first_shown(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        host.place_root(cx, "tabs", "riso.tabs");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    click(&mut visual, "TAB-b");
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("b"))
    );
    // `b` becomes empty: its tab is dropped and `a`, the first shown child,
    // is selected.
    set_empty(&mut app, &views, "b", true);
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-b").is_none(),
        "b's tab is dropped while it is empty"
    );
    assert!(visual.debug_bounds("TAB-a").is_some());
    assert!(visual.debug_bounds("TAB-c").is_some());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("a"))
    );
    assert!(app.update(|cx| shell::is_empty(cx, "b")));
    assert!(!app.update(|cx| shell::is_hidden(cx, "b")));
    // A child that is not selected only loses its tab.
    set_empty(&mut app, &views, "c", true);
    draw(window, &mut app);
    assert!(visual.debug_bounds("TAB-c").is_none());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("a"))
    );
    // `b` shown again: its tab is back, and it is not selected on its own.
    set_empty(&mut app, &views, "b", false);
    draw(window, &mut app);
    assert!(visual.debug_bounds("TAB-b").is_some());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("a"))
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// An empty node is not hidden: `is_hidden` is false, feature 8's operations
// still see it as shown, it has no collapsed line to drag out, and becoming
// empty records nothing.
#[gpui::test]
fn an_empty_node_is_not_hidden_has_no_collapsed_line_and_records_nothing(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "plain", "tour.plain", "root", "children");
        host.place(cx, "b", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    click(&mut visual, "CARD-a");
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // `a` becomes empty: the focus leaves it, as it does a hidden node.
    set_empty(&mut app, &views, "a", true);
    draw(window, &mut app);
    draw(window, &mut app);
    assert_eq!(focused(&mut visual), None);
    assert!(app.update(|cx| shell::is_empty(cx, "a")));
    assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
    // Becoming empty records nothing.
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // `a` leaves no room and no line: `plain` starts at the row's left edge,
    // and a drag there pulls nothing out.
    let body = app.update(|cx| shell::node_bounds(cx, "body")).unwrap();
    let plain = card(&mut visual, "CARD-plain").expect("plain draws");
    assert_eq!(
        plain.origin.x, body.origin.x,
        "plain is at the row's left edge"
    );
    let at = gpui::point(body.origin.x + px(1.), plain.center().y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    // A point away from `a`'s old share of the row: a line dragged there
    // would change the shares.
    visual.simulate_mouse_move(
        gpui::point(px(600.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        gpui::point(px(600.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app);
    assert_eq!(
        drawn_width(&mut visual, "CARD-a"),
        px(0.),
        "no collapsed line drags `a` back"
    );
    assert_eq!(
        card(&mut visual, "CARD-plain"),
        Some(plain),
        "the drag changed none of the shares"
    );
    assert!(app.update(|cx| shell::is_empty(cx, "a")));
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // `plain`'s component never said it can be empty: its view saying so
    // changes nothing.
    set_empty(&mut app, &views, "plain", true);
    draw(window, &mut app);
    assert!(!app.update(|cx| shell::is_empty(cx, "plain")));
    assert!(
        card(&mut visual, "CARD-plain").is_some(),
        "plain keeps its room"
    );
    // Feature 8's operations see `a` as shown: hiding it is a real
    // operation, and showing it again is too.
    app.update(|cx| host.set_hidden(cx, "a", true));
    assert!(app.update(|cx| shell::is_hidden(cx, "a")));
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    app.update(|cx| host.set_hidden(cx, "a", false));
    assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 2);
    assert!(app.update(|cx| shell::is_empty(cx, "a")));
    assert!(errors().is_empty(), "{:?}", errors());
}

// A node that is both hidden and empty stays hidden when it stops being
// empty.
#[gpui::test]
fn a_hidden_node_that_stops_being_empty_stays_hidden(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "b", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    app.update(|cx| host.set_hidden(cx, "a", true));
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    // Hidden and empty, then no longer empty: hidden either way.
    set_empty(&mut app, &views, "a", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_hidden(cx, "a")));
    assert!(app.update(|cx| shell::is_empty(cx, "a")));
    set_empty(&mut app, &views, "a", false);
    draw(window, &mut app);
    assert!(
        app.update(|cx| shell::is_hidden(cx, "a")),
        "a stays hidden when it stops being empty"
    );
    assert!(!app.update(|cx| shell::is_empty(cx, "a")));
    // Only the hide was recorded; the emptiness changes record nothing.
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    // A hidden child keeps the split's collapsed line where it was: `a` is
    // the first child, so its line is at the row's left edge, inside `b`'s
    // leading edge. Dragging it out shows `a` again.
    let b = card(&mut visual, "CARD-b").expect("b draws");
    let at = gpui::point(b.left(), b.center().y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(px(300.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        gpui::point(px(300.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app);
    assert!(
        !app.update(|cx| shell::is_hidden(cx, "a")),
        "a's collapsed line drags it back out"
    );
    let a = card(&mut visual, "CARD-a").expect("a draws again");
    assert!(
        (f32::from(a.size.width) - 300.).abs() < 1.,
        "a is shown at the width the line was dragged to: {a:?}"
    );
    // The drag recorded the split's new shares and the show; the emptiness
    // changes still recorded nothing.
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 3);
    assert!(errors().is_empty(), "{:?}", errors());
}

// `register_empty` can be called before or after the component is registered.
#[gpui::test]
fn register_empty_before_and_after_register_both_work(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        // tour.early says so before registering, tour.late after.
        host.register_empty::<Card>(cx, "tour.early");
        let views_early = views.clone();
        host.register(cx, "tour.early", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                empty: false,
            });
            views_early
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        let views_late = views.clone();
        host.register(cx, "tour.late", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                empty: false,
            });
            views_late
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        host.register_empty::<Card>(cx, "tour.late");
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "early", "tour.early", "root", "children");
        host.place(cx, "late", "tour.late", "root", "children");
    });
    let window = open_and_draw(&mut app);
    assert!(errors().is_empty(), "no ERRORs: {:?}", errors());
    // Both declarations took: each node's view can say it is empty.
    set_empty(&mut app, &views, "early", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_empty(cx, "early")));
    assert!(!app.update(|cx| shell::is_empty(cx, "late")));
    set_empty(&mut app, &views, "late", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_empty(cx, "late")));
    set_empty(&mut app, &views, "early", false);
    draw(window, &mut app);
    assert!(!app.update(|cx| shell::is_empty(cx, "early")));
    assert!(errors().is_empty(), "{:?}", errors());
}

// `register_empty` is refused for an invalid ID, a component outside the
// extension's namespace, and a second declaration — each with an ERROR.
#[gpui::test]
fn register_empty_is_refused_for_an_invalid_id_another_extensions_component_and_a_second_time(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, views, _starts) = setup(&mut app);
    app.update(|cx| {
        host.register_empty::<Card>(cx, "bad component");
        host.register_empty::<Card>(cx, "guest.card");
        // tour.card already said so in the setup.
        host.register_empty::<Card>(cx, "tour.card");
    });
    let errors = errors();
    assert_eq!(errors.len(), 3, "{errors:?}");
    for component in ["bad component", "guest.card", "tour.card"] {
        assert!(
            errors.iter().any(|message| message.contains(component)),
            "a refusal names {component}: {errors:?}"
        );
    }
    // The declaration that was not refused still works.
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "b", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    set_empty(&mut app, &views, "a", true);
    draw(window, &mut app);
    assert!(app.update(|cx| shell::is_empty(cx, "a")));
}
