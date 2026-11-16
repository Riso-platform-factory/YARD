use gpui::{
    AppContext, Bounds, IntoElement, ParentElement, Pixels, Point, Styled, TestAppContext, px,
};
use riso_layout::{NodeId, Operation, Position, decode};
use riso_shell as shell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-split-on-drop-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Every test in this file shares one process-wide diagnostic logger and runs
// under one lock, so a test never sees another test's records, and a test
// that captures the lifecycle trace never runs beside another that reaches
// the same lines.
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

type Shown = Rc<RefCell<Vec<(String, Bounds<Pixels>)>>>;

// A card fills the zone its container gives it and draws the views of its
// `body` slot; it records its node and bounds each time it is drawn.
struct Card {
    node: String,
    shown: Shown,
    slots: shell::Slots,
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

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext, shown: &Shown) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app, shown);
    window
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

// The value of `name=` in `line`, without any quotes.
fn field(line: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=");
    let start = line.find(&key)? + key.len();
    let rest = &line[start..];
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(rest[..end].trim_matches('"').to_string())
}

// The group an event line belongs to.
fn group_of(line: &str) -> &str {
    line.split_once(": ").map_or(line, |(group, _)| group)
}

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

fn assert_bounds_near(actual: Bounds<Pixels>, expected: Bounds<Pixels>, what: &str) {
    for (name, actual, expected) in [
        ("origin.x", actual.origin.x, expected.origin.x),
        ("origin.y", actual.origin.y, expected.origin.y),
        ("size.width", actual.size.width, expected.size.width),
        ("size.height", actual.size.height, expected.size.height),
    ] {
        assert!(
            (f32::from(actual) - f32::from(expected)).abs() <= 1.,
            "{what}: {name} differs, {actual:?} != {expected:?}"
        );
    }
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

// Drags the tab `from_selector` onto `to`, checks that the highlight of the
// zone under the pointer draws, and drops. An edge zone highlights the half
// of the node on the edge's side; `expected_overlay` gives that half's bounds
// when they are known.
fn drag_tab(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    visual: &mut gpui::VisualTestContext,
    shown: &Shown,
    from_selector: &'static str,
    to: Point<Pixels>,
    expected_overlay: (&'static str, Option<Bounds<Pixels>>),
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
    let (selector, expected) = expected_overlay;
    let actual = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing overlay {selector}"));
    if let Some(expected) = expected {
        assert_bounds_near(actual, expected, selector);
    }
    visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
    draw(window, app, shown);
    assert_no_overlays(visual);
}

// `host` registers `tour.card` (with a `body` slot) and `place` builds the
// default layout. A node's tab is `TAB-<node>` and its bar `TABBAR-<node>`.
fn setup(
    app: &mut TestAppContext,
    place: impl FnOnce(&shell::Extension, &mut gpui::App),
) -> (shell::Extension, Shown) {
    let shown = Shown::default();
    let cards = shown.clone();
    let host = app.update(move |cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &["body"], move |node, slots, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
                shown: cards.clone(),
                slots,
            })
            .into()
        });
        place(&host, cx);
        host
    });
    (host, shown)
}

// Two tabs containers in a row: `left` has `a`, `b`; `right` has `c`, `d`.
fn two_tabs(host: &shell::Extension, cx: &mut gpui::App) {
    host.place_root(cx, "root", "riso.row");
    host.place(cx, "left", "riso.tabs", "root", "children");
    host.place(cx, "right", "riso.tabs", "root", "children");
    for (node, parent) in [("a", "left"), ("b", "left"), ("c", "right"), ("d", "right")] {
        host.place(cx, node, "tour.card", parent, "children");
    }
}

// `two_tabs` with a plain card `x` between the two containers.
fn card_in_a_row(host: &shell::Extension, cx: &mut gpui::App) {
    host.place_root(cx, "root", "riso.row");
    host.place(cx, "left", "riso.tabs", "root", "children");
    host.place(cx, "x", "tour.card", "root", "children");
    host.place(cx, "right", "riso.tabs", "root", "children");
    for (node, parent) in [("a", "left"), ("b", "left"), ("c", "right"), ("d", "right")] {
        host.place(cx, node, "tour.card", parent, "children");
    }
}

// `branch` is a tab of `root` whose `body` holds a column over `pad` and
// `leaf`, so `leaf` is in `branch`'s own subtree.
fn cycle_tree(host: &shell::Extension, cx: &mut gpui::App) {
    host.place_root(cx, "root", "riso.tabs");
    host.place(cx, "branch", "tour.card", "root", "children");
    host.place(cx, "other", "tour.card", "root", "children");
    host.place(cx, "inner", "riso.column", "branch", "body");
    host.place(cx, "pad", "tour.card", "inner", "children");
    host.place(cx, "leaf", "tour.card", "inner", "children");
}

// A tab dropped on the top edge of a card in a row makes a `riso.column` in
// the card's place, with a new `riso.tabs` holding the tab above the card;
// the row's other children are unchanged.
#[gpui::test]
fn a_tab_dropped_on_the_top_edge_of_a_card_in_a_row_makes_a_column(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, card_in_a_row);
    let window = open_and_draw(&mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "x", "c"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let x_before = card_bounds(&shown, "x");
    let a_before = card_bounds(&shown, "a");
    let c_before = card_bounds(&shown, "c");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        gpui::point(x_before.center().x, x_before.top() + px(8.)),
        (
            "DROP-OVERLAY-top",
            Some(Bounds {
                origin: x_before.origin,
                size: gpui::size(x_before.size.width, x_before.size.height / 2.),
            }),
        ),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "x", "c"]);
    // The new column stands where `x` stood; the new tabs takes its top half.
    let split = app
        .update(|cx| shell::node_bounds(cx, "split-1"))
        .expect("the new split draws");
    assert_bounds_near(split, x_before, "the column takes the card's slot");
    let tabs = app
        .update(|cx| shell::node_bounds(cx, "tabs-1"))
        .expect("the new tabs draws");
    let x = app.update(|cx| shell::node_bounds(cx, "x")).unwrap();
    assert_bounds_near(
        tabs,
        Bounds {
            origin: split.origin,
            size: gpui::size(split.size.width, split.size.height / 2.),
        },
        "the new tabs is the column's top half",
    );
    assert_bounds_near(
        x,
        Bounds {
            origin: gpui::point(split.origin.x, split.origin.y + split.size.height / 2.),
            size: gpui::size(split.size.width, split.size.height / 2.),
        },
        "the card is the column's bottom half",
    );
    assert!(
        card_bounds(&shown, "b").center().y < card_bounds(&shown, "x").center().y,
        "b's card is above x's"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs-1")),
        Some(id("b"))
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
    // The row's other children are unchanged.
    assert_bounds_near(card_bounds(&shown, "a"), a_before, "left is unchanged");
    assert_bounds_near(card_bounds(&shown, "c"), c_before, "right is unchanged");
    assert!(errors().is_empty(), "{:?}", errors());
}

// The left edge of a card in a row moves the tab into the row before the
// card, as feature 9.
#[gpui::test]
fn the_left_edge_of_a_card_in_a_row_moves_the_tab_into_the_row_before_the_card(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, card_in_a_row);
    let window = open_and_draw(&mut app, &shown);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let x_before = card_bounds(&shown, "x");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(x_before.left() + px(8.), x_before.center().y),
        (
            "DROP-OVERLAY-left",
            Some(Bounds {
                origin: x_before.origin,
                size: gpui::size(x_before.size.width / 2., x_before.size.height),
            }),
        ),
    );
    // `a` is a child of the row just before `x`: one move, no new split.
    assert_eq!(shown_nodes(&shown), ["b", "a", "x", "c"]);
    let a = card_bounds(&shown, "a");
    let x = card_bounds(&shown, "x");
    let left = app.update(|cx| shell::node_bounds(cx, "left")).unwrap();
    assert!(
        a.origin.x >= left.right() - px(1.) && a.origin.x + a.size.width <= x.origin.x + px(1.),
        "a sits in the row between left and x: {a:?} between {left:?} and {x:?}"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "left")),
        Some(id("b"))
    );
    assert!(app.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    assert!(errors().is_empty(), "{:?}", errors());
}

// An edge of a tab's content in a tabs container splits around the tabs
// container, not around the shown child.
#[gpui::test]
fn the_bottom_edge_of_a_tabs_content_splits_around_the_tabs_container(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, two_tabs);
    let window = open_and_draw(&mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "c"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let right_before = app.update(|cx| shell::node_bounds(cx, "right")).unwrap();
    let c = card_bounds(&shown, "c");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        gpui::point(c.center().x, c.bottom() - px(8.)),
        ("DROP-OVERLAY-bottom", None),
    );
    assert_eq!(shown_nodes(&shown), ["a", "c", "b"]);
    // The split wraps `right`, which keeps its tabs: `c` is still its
    // selected tab and `d` is still beside it.
    let split = app
        .update(|cx| shell::node_bounds(cx, "split-1"))
        .expect("the new split draws");
    assert_bounds_near(split, right_before, "the column takes the container's slot");
    let right = app.update(|cx| shell::node_bounds(cx, "right")).unwrap();
    let tabs = app
        .update(|cx| shell::node_bounds(cx, "tabs-1"))
        .expect("the new tabs draws");
    assert_bounds_near(
        right,
        Bounds {
            origin: split.origin,
            size: gpui::size(split.size.width, split.size.height / 2.),
        },
        "right is the column's top half",
    );
    assert_bounds_near(
        tabs,
        Bounds {
            origin: gpui::point(split.origin.x, split.origin.y + split.size.height / 2.),
            size: gpui::size(split.size.width, split.size.height / 2.),
        },
        "the new tabs is the column's bottom half",
    );
    assert!(
        card_bounds(&shown, "b").center().y > card_bounds(&shown, "c").center().y,
        "b's card is below c's"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "right")),
        Some(id("c")),
        "c stays a tab of right"
    );
    assert!(
        visual.debug_bounds("TAB-d").is_some(),
        "d stays a tab of right"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs-1")),
        Some(id("b"))
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
    assert!(errors().is_empty(), "{:?}", errors());
}

// The center of a tabs container's content moves the tab into it, last,
// selected.
#[gpui::test]
fn the_center_of_a_tabs_content_moves_the_tab_into_it_last_and_selected(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, two_tabs);
    let window = open_and_draw(&mut app, &shown);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let c = card_bounds(&shown, "c");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        c.center(),
        ("DROP-OVERLAY-center", None),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b"]);
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "right")),
        Some(id("b")),
        "the moved tab is selected"
    );
    // `b` lands last in `right`'s children: its tab follows `d`'s.
    assert!(
        visual.debug_bounds("TAB-d").unwrap().origin.x
            < visual.debug_bounds("TAB-b").unwrap().origin.x,
        "b's tab is last in right's bar"
    );
    assert!(app.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    assert!(errors().is_empty(), "{:?}", errors());
}

// The four operations are recorded in order, saved, restored, and reset.
#[gpui::test]
fn the_four_operations_are_recorded_in_order_saved_restored_and_reset(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data").join("layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (_host, shown) = setup(&mut app, two_tabs);
        let window = open_and_draw(&mut app, &shown);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let c = card_bounds(&shown, "c");
        drag_tab(
            window,
            &mut app,
            &mut visual,
            &shown,
            "TAB-b",
            gpui::point(c.center().x, c.top() + px(8.)),
            ("DROP-OVERLAY-top", None),
        );
        assert_eq!(shown_nodes(&shown), ["a", "b", "c"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
        // In order: add the new split at the target's index, move the target
        // into it, add the new tabs on the edge's side, move the tab into it.
        let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
        assert_eq!(ops.len(), 4, "{ops:?}");
        match &ops[0] {
            Operation::Add {
                node,
                parent,
                slot,
                at,
            } => {
                assert_eq!(node.id.as_str(), "split-1");
                assert_eq!(node.component.as_str(), "riso.column");
                assert_eq!(node.extension.as_str(), "riso");
                assert_eq!(parent.as_str(), "root");
                assert_eq!(slot.as_str(), "children");
                match at {
                    Position::Index(1) => {}
                    Position::Before(node) => assert_eq!(node.as_str(), "right"),
                    at => panic!("the split takes the target's index in its slot: {at:?}"),
                }
            }
            op => panic!("the new split is added first: {op:?}"),
        }
        match &ops[1] {
            Operation::Move {
                node, parent, slot, ..
            } => {
                assert_eq!(node.as_str(), "right");
                assert_eq!(parent.as_str(), "split-1");
                assert_eq!(slot.as_str(), "children");
            }
            op => panic!("the target is moved into the split second: {op:?}"),
        }
        match &ops[2] {
            Operation::Add {
                node,
                parent,
                slot,
                at,
            } => {
                assert_eq!(node.id.as_str(), "tabs-1");
                assert_eq!(node.component.as_str(), "riso.tabs");
                assert_eq!(node.extension.as_str(), "riso");
                assert_eq!(parent.as_str(), "split-1");
                assert_eq!(slot.as_str(), "children");
                match at {
                    Position::First | Position::Index(0) => {}
                    Position::Before(node) => assert_eq!(node.as_str(), "right"),
                    at => panic!("the new tabs goes on the edge's side: {at:?}"),
                }
            }
            op => panic!("the new tabs is added third: {op:?}"),
        }
        match &ops[3] {
            Operation::Move {
                node, parent, slot, ..
            } => {
                assert_eq!(node.as_str(), "b");
                assert_eq!(parent.as_str(), "tabs-1");
                assert_eq!(slot.as_str(), "children");
            }
            op => panic!("the dropped tab is moved into the new tabs last: {op:?}"),
        }
        let b = card_bounds(&shown, "b");
        let c = card_bounds(&shown, "c");
        assert!(
            b.center().y < c.center().y,
            "b's card is above c's: {b:?} {c:?}"
        );
    }
    // The saved operations restore the same layout at the next start.
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, shown) = setup(&mut app, two_tabs);
        let window = open_and_draw(&mut app, &shown);
        assert_eq!(shown_nodes(&shown), ["a", "b", "c"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "tabs-1")),
            Some(id("b"))
        );
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("c"))
        );
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(shown_nodes(&shown), ["a", "c"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(app.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
        assert!(app.update(|cx| shell::node_bounds(cx, "tabs-1")).is_none());
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("c"))
        );
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A split refused by a rule leaves the tree as it was, with nothing recorded.
#[gpui::test]
fn a_split_refused_by_a_rule_leaves_the_tree_as_it_was_with_nothing_recorded(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let (_host, shown) = setup(&mut app, cycle_tree);
        let window = open_and_draw(&mut app, &shown);
        assert_eq!(shown_nodes(&shown), ["branch", "pad", "leaf"]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let leaf = card_bounds(&shown, "leaf");
        let before = trace.0.lock().unwrap().lines.len();
        // `leaf`'s left edge is across `inner`'s column axis, so the drop makes
        // a `riso.row` around `leaf`; `leaf` is in `branch`'s own subtree
        // through `inner`, so moving `branch` into the new tabs is a cycle and
        // the drop is undone.
        drag_tab(
            window,
            &mut app,
            &mut visual,
            &shown,
            "TAB-branch",
            gpui::point(leaf.left() + px(8.), leaf.center().y),
            ("DROP-OVERLAY-left", None),
        );
        assert_eq!(shown_nodes(&shown), ["branch", "pad", "leaf"]);
        assert_bounds_near(card_bounds(&shown, "leaf"), leaf, "the tree is as it was");
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(app.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
        assert!(app.update(|cx| shell::node_bounds(cx, "tabs-1")).is_none());
        let captured = trace.0.lock().unwrap();
        let lines = &captured.lines[before..];
        let refusals: Vec<&String> = lines.iter().filter(|line| line.contains("INFO")).collect();
        assert_eq!(
            refusals.len(),
            1,
            "the refusal is logged once: {refusals:?}"
        );
        let group = group_of(refusals[0]);
        assert!(
            group.starts_with("layout-op") && refusals[0].contains("cycle"),
            "the cycle refusal is in the operation's group: {}",
            refusals[0]
        );
        assert_eq!(field(group, "node").as_deref(), Some("branch"), "{group}");
        let undone: Vec<&String> = lines
            .iter()
            .filter(|line| line.contains("split on drop undone"))
            .collect();
        assert_eq!(undone.len(), 1, "the undo is logged once: {undone:?}");
        assert!(
            undone[0].contains("DEBUG") && field(undone[0], "done").as_deref() == Some("3"),
            "the three applied operations are undone at debug: {}",
            undone[0]
        );
    });
    assert!(trace.0.lock().unwrap().entered.is_empty());
    assert!(errors().is_empty(), "{:?}", errors());
}

// New node IDs are `split-<n>` and `tabs-<n>`, with `n` the smallest number
// for which the ID is free.
#[gpui::test]
fn new_node_ids_are_the_smallest_free_split_and_tabs_numbers(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, |host, cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "left", "riso.tabs", "root", "children");
        host.place(cx, "x", "tour.card", "root", "children");
        for node in ["a", "b"] {
            host.place(cx, node, "tour.card", "left", "children");
        }
        // `split-1`, `tabs-1` and `tabs-2` are taken; hidden, they stay taken.
        for (node, component) in [
            ("split-1", "riso.row"),
            ("tabs-1", "riso.tabs"),
            ("tabs-2", "riso.tabs"),
        ] {
            host.place(cx, node, component, "root", "children");
            host.set_hidden(cx, node, true);
        }
    });
    let window = open_and_draw(&mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "x"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let x = card_bounds(&shown, "x");
    // The first free split number is 2; the first free tabs number is 3.
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-b",
        gpui::point(x.center().x, x.top() + px(8.)),
        ("DROP-OVERLAY-top", None),
    );
    assert_eq!(shown_nodes(&shown), ["a", "b", "x"]);
    assert!(app.update(|cx| shell::node_bounds(cx, "split-2")).is_some());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs-3")),
        Some(id("b"))
    );
    assert!(app.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
    assert!(app.update(|cx| shell::node_bounds(cx, "tabs-1")).is_none());
    assert!(app.update(|cx| shell::node_bounds(cx, "tabs-2")).is_none());
    // The next split-on-drop continues: `x`'s left edge is across `split-2`'s
    // vertical axis, so a `riso.row` wraps it with `a` in `tabs-4` on the left.
    let x = card_bounds(&shown, "x");
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-a",
        gpui::point(x.left() + px(8.), x.center().y),
        ("DROP-OVERLAY-left", None),
    );
    assert_eq!(shown_nodes(&shown), ["b", "a", "x"]);
    assert!(app.update(|cx| shell::node_bounds(cx, "split-3")).is_some());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs-4")),
        Some(id("a"))
    );
    // `left` was left empty by the drops and stays, as the contract says.
    assert!(app.update(|cx| shell::node_bounds(cx, "left")).is_some());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 8);
    assert!(errors().is_empty(), "{:?}", errors());
}
