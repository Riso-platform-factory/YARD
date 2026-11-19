use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_layout::{Operation, decode};
use yard_shell as shell;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-snap-hide-{}-{}-{nanos:x}",
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
// under one lock, so a test never sees another test's records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
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

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let guard = LOCK.lock().unwrap();
    RECORDS.lock().unwrap().clear();
    guard
}

fn records() -> Vec<(log::Level, String)> {
    RECORDS.lock().unwrap().clone()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// A card fills the zone its split gives it and is found as `CARD-<node>`.
struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

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

// `host` registers the one component of these tests.
fn extension(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host
    })
}

// The layout of these tests: `side`, a card fixed at 240 px, beside `main`.
fn place_default(app: &mut TestAppContext, host: &shell::Extension) {
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
        host.place(cx, "side", "tour.card", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
}

fn widths(visual: &mut gpui::VisualTestContext, selectors: &[&'static str]) -> Vec<gpui::Pixels> {
    selectors
        .iter()
        .copied()
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("missing {selector}"))
                .size
                .width
        })
        .collect()
}

// The width a card is drawn at, or 0 when it is not drawn at all: a hidden
// child of a split leaves only a thin line at its place.
fn shown_len(visual: &mut gpui::VisualTestContext, selector: &'static str) -> f32 {
    visual
        .debug_bounds(selector)
        .map_or(0., |bounds| f32::from(bounds.size.width))
}

// The handle's 4px hitbox is centered on the child's edge, so the edge itself
// is inside it.
fn handle(visual: &mut gpui::VisualTestContext, before: &'static str) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(before).expect("the child draws");
    gpui::point(bounds.right(), bounds.center().y)
}

// The collapsed line of a hidden first child sits at the split's left edge,
// between the edge and the next child: the next child's leading edge is
// inside its hitbox.
fn collapsed_line(
    visual: &mut gpui::VisualTestContext,
    next: &'static str,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(next).expect("the next child draws");
    gpui::point(bounds.left(), bounds.center().y)
}

fn saved_ops(layout_path: &std::path::Path) -> Vec<Operation> {
    decode(&fs::read_to_string(layout_path).unwrap()).unwrap()
}

fn is_hide(operation: &Operation, node: &str) -> bool {
    matches!(operation, Operation::SetHidden { node: id, hidden: true } if id.as_str() == node)
}

fn is_show(operation: &Operation, node: &str) -> bool {
    matches!(operation, Operation::SetHidden { node: id, hidden: false } if id.as_str() == node)
}

fn is_set_props_on(operation: &Operation, node: &str) -> bool {
    matches!(operation, Operation::SetProps { node: id, .. } if id.as_str() == node)
}

// A fixed child of a split dragged past half its minimum is hidden with one
// `hide` operation at the mouse up, and keeps its entry in the split's
// `sizes`, so showing it again brings it back at 240 px.
#[gpui::test]
fn a_side_card_dragged_shut_is_hidden_with_one_hide_and_keeps_its_size(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    place_default(&mut app, &host);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // Dragged past half the minimum and back before releasing, nothing is
    // recorded.
    let at = handle(&mut visual, "CARD-side");
    let back = gpui::point(px(240.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(px(30.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_move(back, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(back, gpui::MouseButton::Left, Default::default());
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "a snap taken back before the mouse up records nothing"
    );
    assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // Dragged past half the minimum and released, `side` is hidden.
    let at = handle(&mut visual, "CARD-side");
    let target = gpui::point(px(30.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded while the mouse is down"
    );
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    // Only a thin line stays at the left edge; `main` has the room.
    assert!(shown_len(&mut visual, "CARD-side") <= 4., "{:?}", records());
    assert!(
        shown_len(&mut visual, "CARD-main") > 890.,
        "main is {}",
        shown_len(&mut visual, "CARD-main")
    );
    // One `hide` operation; the only other operation a snap may record is the
    // split's resize `set-props`, which keeps `side`'s entry in `sizes`.
    let ops = saved_ops(&layout_path);
    assert_eq!(
        ops.iter().filter(|op| is_hide(op, "side")).count(),
        1,
        "{ops:?}"
    );
    assert!(
        ops.len() <= 2,
        "one snap is at most a set-props and a hide: {ops:?}"
    );
    for op in ops.iter().filter(|op| !is_hide(op, "side")) {
        match op {
            Operation::SetProps { node, props } => {
                assert_eq!(node.as_str(), "root", "{op:?}");
                if let Some(sizes) = props.get("sizes").and_then(serde_json::Value::as_object) {
                    let side = sizes
                        .get("side")
                        .and_then(serde_json::Value::as_f64)
                        .expect("side's entry in sizes is kept");
                    assert!((side - 240.).abs() < 0.01, "side's entry is {side}");
                }
            }
            other => panic!("expected at most a set-props beside the hide, got {other:?}"),
        }
    }
    // Its entry is kept: shown again, `side` comes back at 240 px.
    app.update(|cx| host.set_hidden(cx, "side", false));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// The collapsed line left by a snapped-shut child, dragged out past half its
// minimum, shows it again with one `show` operation at the mouse up.
#[gpui::test]
fn the_collapsed_line_dragged_out_shows_the_child_with_one_show(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    place_default(&mut app, &host);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // Snap `side` shut first.
    let at = handle(&mut visual, "CARD-side");
    let target = gpui::point(px(30.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    // A drag of the line that stays under half the minimum leaves it hidden.
    let at = collapsed_line(&mut visual, "CARD-main");
    let target = gpui::point(px(20.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(app.update(|cx| shell::is_hidden(cx, "side")));
    let before = app.update(|cx| shell::user_layer_len(cx));
    // Dragged out past half the minimum, `side` is shown again.
    let at = collapsed_line(&mut visual, "CARD-main");
    let target = gpui::point(px(300.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        before,
        "nothing is recorded while the mouse is down"
    );
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    // It comes back at its kept size or the size dragged to.
    let side = shown_len(&mut visual, "CARD-side");
    assert!(
        (side - 240.).abs() < 1. || (side - 300.).abs() < 1.,
        "side is {side}"
    );
    let ops = saved_ops(&layout_path);
    assert_eq!(
        ops.iter().filter(|op| is_show(op, "side")).count(),
        1,
        "{ops:?}"
    );
    for op in ops
        .iter()
        .filter(|op| !is_hide(op, "side") && !is_show(op, "side"))
    {
        assert!(is_set_props_on(op, "root"), "{op:?}");
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A required node is never snapped shut: the drag stops at its minimum as
// before, and no `hide` is recorded.
#[gpui::test]
fn a_required_node_is_never_snapped_shut(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    app.update(|cx| host.require(cx, "tour.card"));
    place_default(&mut app, &host);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // Dragging `side`'s handle far past half its minimum only resizes: the
    // drag stops at `side`'s minimum, 80 px at the 16 px font.
    let at = handle(&mut visual, "CARD-side");
    let target = gpui::point(px(10.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(80.), px(820.)]
    );
    // A plain resize happened: a set-props records the size at the minimum,
    // and no `hide` is among the operations.
    let ops = saved_ops(&layout_path);
    assert!(
        !ops.iter()
            .any(|op| is_hide(op, "side") || is_show(op, "side")),
        "{ops:?}"
    );
    assert_eq!(ops.len(), 1, "{ops:?}");
    match &ops[0] {
        Operation::SetProps { node, props } => {
            assert_eq!(node.as_str(), "root");
            let sizes = props
                .get("sizes")
                .and_then(serde_json::Value::as_object)
                .expect("the resize records sizes");
            let side = sizes
                .get("side")
                .and_then(serde_json::Value::as_f64)
                .expect("side's recorded size is a number");
            assert!((side - 80.).abs() < 0.01, "side's recorded size is {side}");
        }
        other => panic!("expected a set-props, got {other:?}"),
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A child hidden by `set_hidden` gets the same collapsed line, which drags
// it back.
#[gpui::test]
fn a_child_hidden_by_set_hidden_shows_a_collapsed_line_that_brings_it_back(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    place_default(&mut app, &host);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    app.update(|cx| host.set_hidden(cx, "side", true));
    draw(window, &mut app);
    assert!(
        shown_len(&mut visual, "CARD-side") <= 4.,
        "only a line stays at the left edge"
    );
    assert!(shown_len(&mut visual, "CARD-main") > 890.);
    // The line drags `side` back, as if it had been snapped shut.
    let at = collapsed_line(&mut visual, "CARD-main");
    let target = gpui::point(px(300.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
    draw(window, &mut app);
    let side = shown_len(&mut visual, "CARD-side");
    assert!(
        (side - 240.).abs() < 1. || (side - 300.).abs() < 1.,
        "side is {side}"
    );
    let ops = saved_ops(&layout_path);
    assert_eq!(
        ops.iter().filter(|op| is_hide(op, "side")).count(),
        1,
        "{ops:?}"
    );
    assert_eq!(
        ops.iter().filter(|op| is_show(op, "side")).count(),
        1,
        "{ops:?}"
    );
    for op in ops
        .iter()
        .filter(|op| !is_hide(op, "side") && !is_show(op, "side"))
    {
        assert!(is_set_props_on(op, "root"), "{op:?}");
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// The snaps are saved and restored with the user layer: at the next start
// the child is still hidden behind its line, the line brings it back, and a
// reset shows it again.
#[gpui::test]
fn a_snap_is_saved_and_restored_and_a_reset_shows_the_child_again(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data/layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let host = extension(&mut app);
        place_default(&mut app, &host);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let at = handle(&mut visual, "CARD-side");
        let target = gpui::point(px(30.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert!(app.update(|cx| shell::is_hidden(cx, "side")));
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.iter().filter(|op| is_hide(op, "side")).count(),
            1,
            "{ops:?}"
        );
    }
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let host = extension(&mut app);
        place_default(&mut app, &host);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // The saved `hide` is restored: `side` is still hidden, behind its
        // line at the left edge.
        assert!(app.update(|cx| shell::is_hidden(cx, "side")));
        assert!(shown_len(&mut visual, "CARD-side") <= 4.);
        assert!(shown_len(&mut visual, "CARD-main") > 890.);
        // The line drags it out at 240 px.
        let at = collapsed_line(&mut visual, "CARD-main");
        let target = gpui::point(px(240.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
        draw(window, &mut app);
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(240.), px(660.)]
        );
        // A reset drops the user layer: `side` is shown again at 240 px.
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(!app.update(|cx| shell::is_hidden(cx, "side")));
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(240.), px(660.)]
        );
    }
    assert!(errors().is_empty(), "{:?}", errors());
}
