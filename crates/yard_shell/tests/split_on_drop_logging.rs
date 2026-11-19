use gpui::{
    AppContext, Bounds, IntoElement, ParentElement, Pixels, Point, Styled, TestAppContext, px,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::{Arc, Mutex, Once},
};
use yard_shell as shell;

// One process-wide diagnostic logger, installed once: this file's one test
// never sees another test's records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
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

fn diagnostics() {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
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
        if attributes.metadata().target().starts_with("yard_shell") {
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
        if !event.metadata().target().starts_with("yard_shell")
            && !event.metadata().target().starts_with("yard_layout")
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

// Drags the tab `from_selector` onto `to`, checks that the zone's highlight
// draws, and drops.
fn drag_tab(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    visual: &mut gpui::VisualTestContext,
    shown: &Shown,
    from_selector: &'static str,
    to: Point<Pixels>,
    expected_overlay: &'static str,
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
    assert!(
        visual.debug_bounds(expected_overlay).is_some(),
        "missing overlay {expected_overlay}"
    );
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
        yard_ui::init(cx);
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
    host.place_root(cx, "root", "yard.row");
    host.place(cx, "left", "yard.tabs", "root", "children");
    host.place(cx, "right", "yard.tabs", "root", "children");
    for (node, parent) in [("a", "left"), ("b", "left"), ("c", "right"), ("d", "right")] {
        host.place(cx, node, "tour.card", parent, "children");
    }
}

// `branch` is a tab of `root` whose `body` holds a column over `pad` and
// `leaf`, so `leaf` is in `branch`'s own subtree.
fn cycle_tree(host: &shell::Extension, cx: &mut gpui::App) {
    host.place_root(cx, "root", "yard.tabs");
    host.place(cx, "branch", "tour.card", "root", "children");
    host.place(cx, "other", "tour.card", "root", "children");
    host.place(cx, "inner", "yard.column", "branch", "body");
    host.place(cx, "pad", "tour.card", "inner", "children");
    host.place(cx, "leaf", "tour.card", "inner", "children");
}

// Alone in this binary: every call reaching the captured lines uses the one
// subscriber.
#[gpui::test]
fn split_on_drop_logs_its_group_zone_changes_and_a_refusal(cx: &mut TestAppContext) {
    diagnostics();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        a_split_on_drop_groups_its_four_operations(cx, &trace);
        a_refused_split_on_drop_is_undone_and_logged(cx, &trace);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    drop(captured);
    assert!(errors().is_empty(), "{:?}", errors());
}

// The contract's logging table, first and third rows: a group
// `split-on-drop{node, target, edge}` holds the four `layout-op` groups of
// the operations, and the zone under the pointer logs `drop zone
// {target, zone}` at trace each time it changes.
fn a_split_on_drop_groups_its_four_operations(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, two_tabs);
    let window = open_and_draw(&mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "c"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let c = card_bounds(&shown, "c");
    let start = visual.debug_bounds("TAB-b").unwrap().center();
    visual.simulate_mouse_move(start, None, Default::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(start.x + px(5.), start.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, &mut app, &shown);
    // The pointer crosses four of `c`'s zones: the top edge, the center, the
    // right edge and the bottom edge; each change logs `drop zone` at trace.
    let hovers = [
        gpui::point(c.center().x, c.top() + px(8.)),
        c.center(),
        gpui::point(c.right() - px(8.), c.center().y),
        gpui::point(c.center().x, c.bottom() - px(8.)),
    ];
    let mut zone_lines: Vec<String> = Vec::new();
    for position in hovers {
        let mark = trace.0.lock().unwrap().lines.len();
        visual.simulate_mouse_move(position, gpui::MouseButton::Left, Default::default());
        draw(window, &mut app, &shown);
        let captured = trace.0.lock().unwrap();
        let lines: Vec<String> = captured.lines[mark..]
            .iter()
            .filter(|line| line.contains("drop zone"))
            .cloned()
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| field(line, "target").as_deref() == Some("right")),
            "the zone under the pointer names its drop target: {lines:?}"
        );
        zone_lines.extend(lines);
    }
    // Dropping on the top edge splits a `yard.column` around `right`.
    visual.simulate_mouse_move(hovers[0], gpui::MouseButton::Left, Default::default());
    draw(window, &mut app, &shown);
    visual.simulate_mouse_up(hovers[0], gpui::MouseButton::Left, Default::default());
    draw(window, &mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["a", "b", "c"]);
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
    let captured = trace.0.lock().unwrap();
    // The group names the dropped node, the split target and the edge.
    let group = captured
        .spans
        .values()
        .find(|label| label.starts_with("split-on-drop"))
        .expect("a split-on-drop group wraps the drop");
    assert_eq!(field(group, "node").as_deref(), Some("b"), "{group}");
    assert_eq!(field(group, "target").as_deref(), Some("right"), "{group}");
    assert!(
        field(group, "edge")
            .is_some_and(|edge| ["top", "before"].contains(&edge.to_lowercase().as_str())),
        "the edge the tab was dropped on: {group}"
    );
    // It holds the four `layout-op` groups of the operations, in order:
    // add the split, move the target, add the tabs, move the node.
    let ops: Vec<&String> = captured
        .span_stacks
        .iter()
        .filter(|stack| {
            stack.len() >= 2
                && stack[stack.len() - 2].starts_with("split-on-drop")
                && stack[stack.len() - 1].starts_with("layout-op")
        })
        .map(|stack| &stack[stack.len() - 1])
        .collect();
    let expected = [
        ("add", "split-1"),
        ("move", "right"),
        ("add", "tabs-1"),
        ("move", "b"),
    ];
    assert_eq!(
        ops.len(),
        expected.len(),
        "the group holds the four operations: {ops:?}"
    );
    for (label, (op, node)) in ops.iter().zip(expected) {
        let label = label.as_str();
        assert_eq!(field(label, "op").as_deref(), Some(op), "{label}");
        assert_eq!(field(label, "node").as_deref(), Some(node), "{label}");
        assert_eq!(
            field(label, "extension").as_deref(),
            Some("yard"),
            "{label}"
        );
    }
    // The zone changes logged `drop zone {target, zone}` at trace.
    assert!(
        zone_lines.len() >= 3,
        "the zone under the pointer logs each change: {zone_lines:?}"
    );
    for line in &zone_lines {
        assert!(line.contains("TRACE"), "drop zone logs at trace: {line}");
        assert!(field(line, "target").is_some(), "{line}");
        assert!(field(line, "zone").is_some(), "{line}");
    }
    let zones: BTreeSet<String> = zone_lines
        .iter()
        .filter_map(|line| field(line, "zone"))
        .collect();
    assert!(
        zones.len() >= 3,
        "the edge zones and the center are distinct zones: {zones:?}"
    );
}

// The contract's logging table, second row: a refusal is one `INFO` in the
// operation's group, and the applied operations are undone with a `debug`
// `split on drop undone {done}`.
fn a_refused_split_on_drop_is_undone_and_logged(cx: &mut TestAppContext, trace: &Trace) {
    let mut app = cx.new_app();
    let (_host, shown) = setup(&mut app, cycle_tree);
    let window = open_and_draw(&mut app, &shown);
    assert_eq!(shown_nodes(&shown), ["branch", "pad", "leaf"]);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let leaf = card_bounds(&shown, "leaf");
    let before = trace.0.lock().unwrap().lines.len();
    // `leaf`'s left edge is across `inner`'s column axis, so the drop makes a
    // `yard.row` around `leaf`; `leaf` is in `branch`'s own subtree, so the
    // last of the four operations, moving `branch` into the new tabs, is
    // refused as a cycle, and the three that ran are undone.
    drag_tab(
        window,
        &mut app,
        &mut visual,
        &shown,
        "TAB-branch",
        gpui::point(leaf.left() + px(8.), leaf.center().y),
        "DROP-OVERLAY-left",
    );
    assert_eq!(shown_nodes(&shown), ["branch", "pad", "leaf"]);
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded"
    );
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
    assert_eq!(field(group, "op").as_deref(), Some("move"), "{group}");
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
}
