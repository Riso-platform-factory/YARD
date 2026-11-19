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
            "yard-shell-fixed-sizes-{}-{}-{nanos:x}",
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

// The handle's 4px hitbox is centered on the child's edge, so the edge itself
// is inside it.
fn handle(visual: &mut gpui::VisualTestContext, before: &'static str) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(before).expect("the child draws");
    gpui::point(bounds.right(), bounds.center().y)
}

// A split placed with `{"sizes": {"side": 240}}` gives `side` 240 px in any
// window wide enough, and `side`'s shares entry is not used while it is fixed.
#[gpui::test]
fn a_split_placed_with_sizes_gives_the_fixed_child_its_size(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(
            cx,
            "root",
            "yard.row",
            json!({"sizes": {"side": 240}, "shares": {"side": 3}}),
        );
        host.place(cx, "side", "tour.card", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // In any window wide enough `side` keeps its size and `main` takes the
    // rest.
    for (width, main) in [(1200., 960.), (700., 460.)] {
        visual.simulate_resize(gpui::size(px(width), px(600.)));
        draw(window, &mut app);
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(240.), px(main)]
        );
    }
    // Too narrow for the fixed size and the minimum: `main` stops at its
    // minimum and `side` shrinks.
    visual.simulate_resize(gpui::size(px(300.), px(600.)));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(220.), px(80.)]
    );
    // Grown again, `side` has its size back.
    visual.simulate_resize(gpui::size(px(900.), px(600.)));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A drag of a fixed child's handle records one set-props with its new size
// under `sizes`.
#[gpui::test]
fn a_drag_of_a_fixed_childs_handle_records_one_set_props_with_its_size(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
        host.place(cx, "side", "tour.card", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let at = handle(&mut visual, "CARD-side");
    // A click on the handle without a move records nothing.
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // Two steps of one drag still join into one operation, at the mouse up.
    let target = gpui::point(px(340.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(px(300.), at.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded while the mouse is down"
    );
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(340.), px(560.)]
    );
    let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Operation::SetProps { node, props } => {
            assert_eq!(node.as_str(), "root");
            let sizes = props
                .get("sizes")
                .and_then(serde_json::Value::as_object)
                .expect("the sizes are an object");
            let side = sizes
                .get("side")
                .and_then(serde_json::Value::as_f64)
                .expect("side's recorded size is a number");
            assert!((side - 340.).abs() < 0.01, "side's recorded size is {side}");
        }
        other => panic!("expected a set-props, got {other:?}"),
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// Sizes are recorded in the user layer, so they are saved and restored at the
// next start, and a reset gives the default layout's sizes.
#[gpui::test]
fn sizes_are_saved_and_restored_and_a_reset_gives_the_default_sizes(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data/layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let host = extension(&mut app);
        app.update(|cx| {
            host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
            host.place(cx, "side", "tour.card", "root", "children");
            host.place(cx, "main", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let at = handle(&mut visual, "CARD-side");
        let target = gpui::point(px(340.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        draw(window, &mut app);
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(340.), px(560.)]
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    }
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let host = extension(&mut app);
        app.update(|cx| {
            host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
            host.place(cx, "side", "tour.card", "root", "children");
            host.place(cx, "main", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // The saved size is restored.
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(340.), px(560.)]
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        // A reset gives the default layout's sizes again.
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        assert_eq!(
            widths(&mut visual, &["CARD-side", "CARD-main"]),
            [px(240.), px(660.)]
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A double click records `{"shares": null}` and leaves `sizes` alone: the
// fixed children stay fixed.
#[gpui::test]
fn a_double_click_leaves_sizes_alone(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
        host.place(cx, "side", "tour.card", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let at = handle(&mut visual, "CARD-side");
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_event(gpui::MouseDownEvent {
        position: at,
        button: gpui::MouseButton::Left,
        click_count: 2,
        modifiers: Default::default(),
        first_mouse: false,
    });
    visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // `side` is still fixed: a wider window does not move it.
    visual.simulate_resize(gpui::size(px(1200.), px(600.)));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(240.), px(960.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Operation::SetProps { node, props } => {
            assert_eq!(node.as_str(), "root");
            assert_eq!(
                serde_json::Value::Object(props.clone()),
                json!({"shares": null})
            );
        }
        other => panic!("expected a set-props, got {other:?}"),
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A size that is not a positive number is read as no entry, with a WARN.
#[gpui::test]
fn a_size_that_is_not_a_positive_number_reads_as_no_entry_with_a_warn(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": "bad"}}));
        host.place(cx, "side", "tour.card", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // `side`'s bad value reads as no entry: both children stay flexible.
    assert_eq!(
        widths(&mut visual, &["CARD-side", "CARD-main"]),
        [px(450.), px(450.)]
    );
    let bad_reads: Vec<String> = records()
        .into_iter()
        .filter(|(level, message)| *level == log::Level::Warn && message.contains("cannot be read"))
        .map(|(_, message)| message)
        .collect();
    assert_eq!(bad_reads.len(), 1, "{bad_reads:?}");
    assert!(
        bad_reads[0].starts_with("prop sizes of node root cannot be read:"),
        "{}",
        bad_reads[0]
    );
    assert!(
        bad_reads[0].ends_with("the default is used"),
        "{}",
        bad_reads[0]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}
