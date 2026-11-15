use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell as shell;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-component-sizes-{}-{}-{nanos:x}",
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

// Every test of this file shares the one diagnostic logger a process may have,
// so each runs under the lock and sees only its own records.
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

// A card fills the zone its split gives it, found as `CARD-<node>`.
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

fn extension(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        shell::Extension::new(cx, "tour").unwrap()
    })
}

// The components of these tests: `card` has no hints, `wide` a wide minimum,
// `panel` a preferred width.
fn register_cards(app: &mut TestAppContext, host: &shell::Extension) {
    app.update(|cx| {
        for component in ["tour.card", "tour.wide", "tour.panel"] {
            host.register(cx, component, &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
        }
    });
}

// Hints of a `min` of `width` by `height` rems, and a `preferred` width of
// `preferred` rems where one is given.
fn hints(width: f32, height: f32, preferred: Option<f32>) -> shell::SizeHints {
    shell::SizeHints {
        min: gpui::size(gpui::rems(width), gpui::rems(height)),
        preferred: gpui::size(preferred.map(gpui::rems), None),
    }
}

fn widths(visual: &mut gpui::VisualTestContext, selectors: &[&'static str]) -> Vec<gpui::Pixels> {
    selectors
        .iter()
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .expect("element must be present")
                .size
                .width
        })
        .collect()
}

// The point where a child's right edge is, which is where the handle of the
// split that follows it sits.
fn handle(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visual
        .debug_bounds(selector)
        .expect("element must be present");
    gpui::point(bounds.right(), bounds.center().y)
}

// A component's minimum in rems stops a drag at that many pixels at the
// window's rem size; a child without hints keeps the default, 5 rems.
#[gpui::test]
fn a_components_minimum_stops_a_drag_and_the_default_is_five_rems(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    register_cards(&mut app, &host);
    app.update(|cx| {
        // Hints may be declared after the component is registered.
        host.size_hints(cx, "tour.wide", hints(20., 6.25, None));
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "wide", "tour.wide", "root", "children");
        host.place(cx, "plain", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(450.), px(450.)]
    );
    // Dragging left, `wide` stops at its 20 rems: 320 px at the 16 px font.
    let at = handle(&mut visual, "CARD-wide");
    let target = gpui::point(px(100.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(320.), px(580.)]
    );
    // Dragging right, `plain` stops at the default 5 rems: 80 px.
    let at = handle(&mut visual, "CARD-wide");
    let target = gpui::point(px(890.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(820.), px(80.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A larger `ui_font_size` moves every minimum on the next frame: the same
// hints stop the same drags at more pixels.
#[gpui::test]
fn a_larger_ui_font_size_moves_the_minimums(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let settings = temp.path().join("settings.json");
    fs::write(&settings, r#"{"theme": {"ui_font_size": 16}}"#).unwrap();
    let mut app = cx.new_app();
    let host = app.update(|cx| {
        riso_settings::init(cx);
        riso_settings::load(cx, settings.clone());
        riso_settings::watch(cx);
        riso_ui::init(cx);
        riso_ui::apply_theme_setting(cx);
        shell::Extension::new(cx, "tour").unwrap()
    });
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.size_hints(cx, "tour.wide", hints(20., 6.25, None));
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "wide", "tour.wide", "root", "children");
        host.place(cx, "plain", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(450.), px(450.)]
    );
    // The user writes a larger UI font size; the next frame's limits follow.
    fs::write(&settings, r#"{"theme": {"ui_font_size": 20}}"#).unwrap();
    app.executor().advance_clock(Duration::from_secs(1));
    app.run_until_parked();
    app.read(|cx| assert_eq!(riso_ui::ui_font_size(cx), px(20.)));
    draw(window, &mut app);
    // The shares did not change: the layout is the same until a drag.
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(450.), px(450.)]
    );
    // `wide` now stops at 20 rems of 20 px: 400 px.
    let at = handle(&mut visual, "CARD-wide");
    let target = gpui::point(px(100.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(400.), px(500.)]
    );
    // and `plain` at the default 5 rems of 20 px: 100 px.
    let at = handle(&mut visual, "CARD-wide");
    let target = gpui::point(px(890.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-wide", "CARD-plain"]),
        [px(800.), px(100.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A split's minimum along its axis is the sum of its shown children's
// minimums: a nested split stops a drag in its parent at that sum.
#[gpui::test]
fn a_nested_split_stops_a_drag_in_its_parent_at_the_sum_of_its_minimums(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.size_hints(cx, "tour.wide", hints(20., 6.25, None));
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "nest", "riso.row", "root", "children");
        host.place(cx, "a", "tour.wide", "nest", "children");
        host.place(cx, "b", "tour.card", "nest", "children");
        host.place(cx, "side", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-side"]),
        [px(320.), px(130.), px(450.)]
    );
    // The handle between `nest` and `side` sits at `nest`'s right edge, where
    // `b` ends.
    let at = handle(&mut visual, "CARD-b");
    let target = gpui::point(px(100.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    // `nest` stops at the sum of its own minimum, 320 + 80 = 400 px; inside it
    // `a` has its 320 and `b` its 80.
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-side"]),
        [px(320.), px(80.), px(500.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A child whose component declares a preferred width starts fixed at it: a
// wider window gives the growth to a sibling, a drag fixes the child at the
// dragged size, and a reset gives the preferred width back.
#[gpui::test]
fn a_child_with_a_preferred_width_starts_fixed_at_it(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        // Hints may be declared before the component is registered.
        host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(15.)));
    });
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "panel", "tour.panel", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // 15 rems at the 16 px font: 240 px.
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(660.)]
    );
    // Fixed: a wider window gives the growth to `main`.
    visual.simulate_resize(gpui::size(px(1200.), px(600.)));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(960.)]
    );
    // A drag fixes `panel` at the dragged size.
    let at = handle(&mut visual, "CARD-panel");
    let target = gpui::point(px(400.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(400.), px(800.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    // A reset gives the preferred width back.
    gpui::AnyWindowHandle::from(window)
        .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
        .unwrap();
    draw(window, &mut app);
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(960.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A `sizes` or `shares` entry in the split's props wins over the preferred
// width.
#[gpui::test]
fn a_sizes_or_shares_entry_wins_over_the_preferred_width(cx: &mut TestAppContext) {
    let _guard = lock();
    for (props, expected) in [
        (json!({"sizes": {"panel": 300}}), [px(300.), px(600.)]),
        (json!({"shares": {"panel": 2}}), [px(600.), px(300.)]),
    ] {
        let mut app = cx.new_app();
        let host = extension(&mut app);
        register_cards(&mut app, &host);
        app.update(move |cx| {
            host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(15.)));
            host.place_root_with_props(cx, "root", "riso.row", props);
            host.place(cx, "panel", "tour.panel", "root", "children");
            host.place(cx, "main", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert_eq!(widths(&mut visual, &["CARD-panel", "CARD-main"]), expected);
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A `sizes` value that cannot be read is no entry: the preferred width still
// applies, with the WARN of the props feature.
#[gpui::test]
fn a_sizes_entry_that_cannot_be_read_leaves_the_preferred_width_with_a_warn(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    register_cards(&mut app, &host);
    app.update(|cx| {
        host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(15.)));
        host.place_root_with_props(cx, "root", "riso.row", json!({"sizes": {"panel": "bad"}}));
        host.place(cx, "panel", "tour.panel", "root", "children");
        host.place(cx, "main", "tour.card", "root", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-panel", "CARD-main"]),
        [px(240.), px(660.)]
    );
    let bad: Vec<String> = records()
        .into_iter()
        .filter(|(level, message)| *level == log::Level::Warn && message.contains("cannot be read"))
        .map(|(_, message)| message)
        .collect();
    assert_eq!(bad.len(), 1, "{:?}", records());
    assert!(
        bad[0].starts_with("prop sizes of node root cannot be read:"),
        "{}",
        bad[0]
    );
    assert!(bad[0].ends_with("the default is used"), "{}", bad[0]);
    assert!(errors().is_empty(), "{:?}", errors());
}

// Every refusal of `size_hints` logs an ERROR in the diagnostic log.
#[gpui::test]
fn each_refusal_of_size_hints_logs_an_error(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    register_cards(&mut app, &host);
    app.update(|cx| {
        // A component ID that cannot be parsed.
        host.size_hints(cx, "a.b.c", hints(5., 6.25, None));
        // A component of another extension's namespace.
        host.size_hints(cx, "other.card", hints(5., 6.25, None));
        // A size that is not positive.
        host.size_hints(cx, "tour.card", hints(0., 6.25, None));
        // A preferred size below the minimum.
        host.size_hints(cx, "tour.panel", hints(5., 6.25, Some(4.)));
        // Hints a component already has.
        host.size_hints(cx, "tour.wide", hints(8., 6.25, None));
        host.size_hints(cx, "tour.wide", hints(10., 6.25, None));
    });
    assert_eq!(errors().len(), 5, "{:?}", errors());
}
