use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_layout::{NodeId, Operation, decode};
use yard_shell as shell;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-close-tab-{}-{}-{nanos:x}",
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
// under one lock, so a test never sees another test's diagnostic records.
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

// The view each node got, so a close can be shown to have dropped it.
type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
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
    draw(window, app);
    window
}

// `host` registers `tour.card` and the required `tour.required`, and places a
// tabs container over three cards and the required `pinned`. A node's tab is
// `TAB-<node>`; the close button its tab gets is `CLOSE-<node>`, as a tab's
// icon is `ICON-<node>`.
fn setup(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        for component in ["tour.card", "tour.required"] {
            let views = views.clone();
            host.register(cx, component, &[], move |node, _, _, cx| {
                let card = cx.new(|_| Card {
                    node: node.to_string(),
                });
                views
                    .borrow_mut()
                    .insert(node.to_string(), card.downgrade());
                card.into()
            });
        }
        host.require(cx, "tour.required");
        host.place_root(cx, "tabs", "yard.tabs");
        host.place(cx, "alpha", "tour.card", "tabs", "children");
        host.place(cx, "beta", "tour.card", "tabs", "children");
        host.place(cx, "delta", "tour.card", "tabs", "children");
        host.place(cx, "pinned", "tour.required", "tabs", "children");
        host
    });
    (host, views)
}

// Clicking a tab's close button removes its node with one `remove`: the tab
// and its view are gone, and since the closed tab was selected, the first
// shown tab is selected.
#[gpui::test]
fn clicking_a_tabs_close_button_removes_its_node_drops_its_view_and_selects_the_first_shown_tab(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // The first shown tab is selected from the start.
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha"))
    );
    let beta = visual
        .debug_bounds("TAB-beta")
        .expect("beta's tab draws")
        .center();
    visual.simulate_click(beta, gpui::Modifiers::default());
    draw(window, &mut app);
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("beta"))
    );
    // The button is drawn only while the pointer is over the tab, so the
    // pointer goes over it first.
    visual.simulate_mouse_move(beta, None, gpui::Modifiers::default());
    draw(window, &mut app);
    let button = visual
        .debug_bounds("CLOSE-beta")
        .expect("the selected tab's close button is drawn")
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-beta").is_none(),
        "the closed tab is gone"
    );
    assert!(
        visual.debug_bounds("CARD-beta").is_none(),
        "the closed node's card is gone"
    );
    assert!(
        views.borrow()["beta"].upgrade().is_none(),
        "the closed node's view is dropped"
    );
    assert!(visual.debug_bounds("TAB-alpha").is_some());
    assert!(visual.debug_bounds("TAB-delta").is_some());
    assert!(visual.debug_bounds("TAB-pinned").is_some());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha")),
        "the first shown tab is selected"
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        1,
        "one remove operation is recorded"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A middle click on a tab removes its node the same way.
#[gpui::test]
fn a_middle_click_on_a_tab_does_the_same(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // On a tab that is not selected: delta goes, the selection stays.
    let delta = visual
        .debug_bounds("TAB-delta")
        .expect("delta's tab draws")
        .center();
    visual.simulate_mouse_down(delta, gpui::MouseButton::Middle, gpui::Modifiers::default());
    visual.simulate_mouse_up(delta, gpui::MouseButton::Middle, gpui::Modifiers::default());
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-delta").is_none(),
        "the closed tab is gone"
    );
    assert!(
        views.borrow()["delta"].upgrade().is_none(),
        "the closed node's view is dropped"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha")),
        "closing an unselected tab does not move the selection"
    );
    // On the selected tab it does the same, and the first shown tab is
    // selected.
    let beta = visual
        .debug_bounds("TAB-beta")
        .expect("beta's tab draws")
        .center();
    visual.simulate_click(beta, gpui::Modifiers::default());
    draw(window, &mut app);
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("beta"))
    );
    visual.simulate_mouse_down(beta, gpui::MouseButton::Middle, gpui::Modifiers::default());
    visual.simulate_mouse_up(beta, gpui::MouseButton::Middle, gpui::Modifiers::default());
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-beta").is_none(),
        "the closed tab is gone"
    );
    assert!(
        visual.debug_bounds("CARD-beta").is_none(),
        "the closed node's card is gone"
    );
    assert!(
        views.borrow()["beta"].upgrade().is_none(),
        "the closed node's view is dropped"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha")),
        "the first shown tab is selected"
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        2,
        "one remove operation per close"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// `pinned` is a required node: its tab has no close button, and a middle
// click on it changes nothing.
#[gpui::test]
fn a_required_nodes_tab_has_no_close_button_and_a_middle_click_on_it_changes_nothing(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert!(
        visual.debug_bounds("CLOSE-alpha").is_some(),
        "a plain tab records its close button's bounds"
    );
    assert!(
        visual.debug_bounds("CLOSE-pinned").is_none(),
        "the required node's tab has no close button"
    );
    let pinned = visual
        .debug_bounds("TAB-pinned")
        .expect("the required tab draws")
        .center();
    visual.simulate_mouse_down(
        pinned,
        gpui::MouseButton::Middle,
        gpui::Modifiers::default(),
    );
    visual.simulate_mouse_up(
        pinned,
        gpui::MouseButton::Middle,
        gpui::Modifiers::default(),
    );
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-pinned").is_some(),
        "the required tab stays"
    );
    assert!(
        views.borrow()["pinned"].upgrade().is_some(),
        "its view is not dropped"
    );
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha")),
        "the selection does not change"
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// The close is recorded in the user layer, so it is saved and restored at the
// next start, and a reset brings the tab back.
#[gpui::test]
fn a_closed_tab_stays_closed_when_the_layout_is_restored_and_comes_back_on_reset(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data").join("layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (_host, _views) = setup(&mut app);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let beta = visual
            .debug_bounds("TAB-beta")
            .expect("beta's tab draws")
            .center();
        visual.simulate_click(beta, gpui::Modifiers::default());
        draw(window, &mut app);
        let button = visual
            .debug_bounds("CLOSE-beta")
            .expect("the selected tab's close button is drawn")
            .center();
        visual.simulate_click(button, gpui::Modifiers::default());
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("TAB-beta").is_none(),
            "the closed tab is gone"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
        assert_eq!(
            ops,
            [Operation::Remove { node: id("beta") }],
            "the close is saved as a remove"
        );
    }
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, views) = setup(&mut app);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // The close is restored: beta stays gone.
        assert!(
            visual.debug_bounds("TAB-beta").is_none(),
            "beta stays closed after the restore"
        );
        assert!(visual.debug_bounds("TAB-alpha").is_some());
        assert!(visual.debug_bounds("TAB-delta").is_some());
        assert!(visual.debug_bounds("TAB-pinned").is_some());
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        // A reset drops the user layer: the tab comes back.
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("TAB-beta").is_some(),
            "beta's tab is back after the reset"
        );
        assert!(
            views
                .borrow()
                .get("beta")
                .is_some_and(|view| view.upgrade().is_some()),
            "beta's view is back"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    }
    assert!(errors().is_empty(), "{:?}", errors());
}
