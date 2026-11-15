use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_layout::{Operation, decode};
use riso_shell as shell;
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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-sizes-as-props-{}-{}-{nanos:x}",
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

// `host` registers the one component of these tests; `guest` is a second
// extension for operations any extension may run.
fn extension(app: &mut TestAppContext) -> (shell::Extension, shell::Extension) {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let guest = shell::Extension::new(cx, "guest").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        (host, guest)
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

// The steps of one drag join into a single set-props at the mouse up, with the
// shares of the shown children keyed by node ID.
#[gpui::test]
fn a_drag_records_one_set_props_with_the_shares_by_node_id(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // Without props the children share the length equally.
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(300.), px(300.), px(300.)]
    );
    let at = handle(&mut visual, "CARD-a");
    // A click on the handle without a move records nothing.
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // Two steps of one drag still join into one operation, at the mouse up.
    let target = gpui::point(px(450.), at.y);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(px(375.), at.y),
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
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(450.), px(150.), px(300.)]
    );
    let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Operation::SetProps { node, props } => {
            assert_eq!(node.as_str(), "root");
            let shares = props
                .get("shares")
                .and_then(serde_json::Value::as_object)
                .expect("the shares are an object");
            assert_eq!(shares.len(), 3);
            assert_eq!(
                shares.get("a").and_then(serde_json::Value::as_f64),
                Some(1.5)
            );
            assert_eq!(
                shares.get("b").and_then(serde_json::Value::as_f64),
                Some(0.5)
            );
            assert_eq!(
                shares.get("c").and_then(serde_json::Value::as_f64),
                Some(1.)
            );
        }
        other => panic!("expected a set-props, got {other:?}"),
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A split placed with `{"shares": {"a": 3}}` gives `a` three times the length
// of its siblings.
#[gpui::test]
fn a_split_placed_with_shares_gives_their_proportions(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "riso.row", json!({"shares": {"a": 3}}));
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(540.), px(180.), px(180.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A share is keyed by node ID: a child moved out of the split keeps its entry,
// and moved back in it has its share again.
#[gpui::test]
fn a_child_moved_out_and_back_has_its_share_again(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "riso.row", json!({"shares": {"b": 3}}));
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
        host.place(cx, "nest", "riso.column", "root", "children");
        host.place(cx, "d", "tour.card", "nest", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c", "CARD-d"]),
        [px(150.), px(450.), px(150.), px(150.)]
    );
    // Moved out, `b`'s entry stays in the props and is ignored by the view.
    app.update(|cx| host.move_node(cx, "b", "nest", "children", 1));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-c", "CARD-d"]),
        [px(300.), px(300.), px(300.)]
    );
    // Moved back, `b` has its share again.
    app.update(|cx| host.move_node(cx, "b", "root", "children", 1));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c", "CARD-d"]),
        [px(150.), px(450.), px(150.), px(150.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A hidden child keeps its entry, and the shown children share the length in
// the proportions of their entries.
#[gpui::test]
fn hiding_a_child_keeps_its_entry(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "riso.row", json!({"shares": {"a": 3, "b": 2}}));
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(450.), px(300.), px(150.)]
    );
    app.update(|cx| host.set_hidden(cx, "b", true));
    draw(window, &mut app);
    assert!(visual.debug_bounds("CARD-b").is_none());
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-c"]),
        [px(675.), px(225.)]
    );
    // Shown again, `b` has its share again.
    app.update(|cx| host.set_hidden(cx, "b", false));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(450.), px(300.), px(150.)]
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// The shares are recorded in the user layer, so they are saved and restored at
// the next start.
#[gpui::test]
fn the_shares_are_saved_and_restored_at_the_next_start(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data/layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, guest) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            for name in ["a", "b", "c"] {
                host.place(cx, name, "tour.card", "root", "children");
            }
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // A set-props from any extension is read by the split.
        app.update(|cx| guest.set_props(cx, "root", json!({"shares": {"b": 3}})));
        draw(window, &mut app);
        assert_eq!(
            widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
            [px(180.), px(540.), px(180.)]
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    }
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, _guest) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            for name in ["a", "b", "c"] {
                host.place(cx, name, "tour.card", "root", "children");
            }
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert_eq!(
            widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
            [px(180.), px(540.), px(180.)]
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    }
    assert!(errors().is_empty(), "{:?}", errors());
}

// A reset drops the recorded operations, so the shares are equal again.
#[gpui::test]
fn a_reset_gives_equal_shares_again(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    app.update(|cx| host.set_props(cx, "root", json!({"shares": {"a": 3}})));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(540.), px(180.), px(180.)]
    );
    gpui::AnyWindowHandle::from(window)
        .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
        .unwrap();
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(300.), px(300.), px(300.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert!(errors().is_empty(), "{:?}", errors());
}

// A double click on a handle records `{"shares": null}`: the shares go back to
// the default, which is equal.
#[gpui::test]
fn a_double_click_records_null_shares_and_gives_equal_shares(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    app.update(|cx| host.set_props(cx, "root", json!({"shares": {"a": 3}})));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(540.), px(180.), px(180.)]
    );
    // The handle between `a` and `b` sits at `a`'s edge.
    let at = handle(&mut visual, "CARD-a");
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
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(300.), px(300.), px(300.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 2);
    let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
    assert_eq!(ops.len(), 2);
    match &ops[1] {
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

// A share that is not a positive number is read as 1, with the WARN of the
// props feature.
#[gpui::test]
fn a_share_that_is_not_a_positive_number_reads_as_1_with_a_warn(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "riso.row", json!({"shares": {"a": "bad"}}));
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // `a`'s bad value reads as 1: all three children stay equal.
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(300.), px(300.), px(300.)]
    );
    let bad_reads: Vec<String> = records()
        .into_iter()
        .filter(|(level, message)| *level == log::Level::Warn && message.contains("cannot be read"))
        .map(|(_, message)| message)
        .collect();
    assert_eq!(bad_reads.len(), 1, "{bad_reads:?}");
    assert!(
        bad_reads[0].starts_with("prop shares of node root cannot be read:"),
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

// A set-props whose props do not encode as a JSON object is an ERROR in the
// diagnostic log and changes nothing.
#[gpui::test]
fn shares_props_that_do_not_encode_as_an_object_are_an_error(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "riso.row");
        for name in ["a", "b", "c"] {
            host.place(cx, name, "tour.card", "root", "children");
        }
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // An extension bug: the props do not encode as a JSON object.
    app.update(|cx| host.set_props(cx, "root", json!([1, 2])));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(300.), px(300.), px(300.)]
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    let errors = errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    // Nothing changed: a good set_props still works afterwards.
    app.update(|cx| host.set_props(cx, "root", json!({"shares": {"b": 2}})));
    draw(window, &mut app);
    assert_eq!(
        widths(&mut visual, &["CARD-a", "CARD-b", "CARD-c"]),
        [px(225.), px(450.), px(225.)]
    );
}
