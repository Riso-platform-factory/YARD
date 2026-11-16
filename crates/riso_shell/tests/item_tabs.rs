use gpui::{
    AppContext, InteractiveElement, IntoElement, SharedString, Styled, TestAppContext, point, px,
};
use riso_shell as shell;
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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-item-tabs-{}-{}-{nanos:x}",
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

// The model the item tabs are over: a list of names. The buttons only ask:
// `close_item` removes the key, except `ada`, which it keeps, and `new_item`
// makes a `new-<n>`; every call is recorded so the test can count it.
struct Names {
    names: Vec<SharedString>,
    closed: Vec<SharedString>,
    added: usize,
}

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.names.clone()
    }
}

impl shell::ItemTabs for Names {
    fn close_item(
        &mut self,
        key: &SharedString,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.closed.push(key.clone());
        if key.as_str() != "ada" {
            self.names.retain(|name| name != key);
        }
        cx.notify();
    }

    fn new_item(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.added += 1;
        self.names.push(format!("new-{}", self.added).into());
        cx.notify();
    }
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

// The view each copy got, so a close can be shown to have dropped it.
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

// `host` places a `riso.tabs` node `tabs` under a `riso.row` root and
// declares its `children` item tabs over the window's `Names`, with one
// `tour.card` template `name`. A copy's tab is `TAB-<copy>` and its close
// button `CLOSE-<copy>`, as a node's are `TAB-<node>` and `CLOSE-<node>`;
// the bar's new-item button is `NEW-<tabs>`, as the bar's drop target is
// `TABBAR-<id>`.
fn setup(app: &mut TestAppContext) -> (shell::Extension, gpui::Entity<Names>, Views) {
    let views = Views::default();
    let (host, model) = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let model = cx.new(|_| Names {
            names: vec!["ada".into(), "bob".into(), "cy".into()],
            closed: Vec::new(),
            added: 0,
        });
        host.provide_window(cx, model.clone());
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
            });
            created
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "tabs", "riso.tabs", "root", "children");
        host.item_tabs::<Names>(cx, "tabs");
        host.place(cx, "name", "tour.card", "tabs", "children");
        (host, model)
    });
    (host, model, views)
}

fn selected(app: &mut TestAppContext) -> Option<String> {
    app.update(|cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
}

// A `riso.tabs` with item tabs over three names shows three tabs, each with
// a close button, and a new button at the bar's end, after the tabs and
// outside the scrolling row.
#[gpui::test]
fn three_items_show_three_tabs_each_with_a_close_button_and_a_new_button_at_the_bars_end(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _model, _views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    for (tab, close) in [
        ("TAB-name@ada", "CLOSE-name@ada"),
        ("TAB-name@bob", "CLOSE-name@bob"),
        ("TAB-name@cy", "CLOSE-name@cy"),
    ] {
        assert!(visual.debug_bounds(tab).is_some(), "{tab} draws");
        assert!(visual.debug_bounds(close).is_some(), "{close} is there");
    }
    assert_eq!(
        selected(&mut app).as_deref(),
        Some("name@ada"),
        "the first item's tab is selected"
    );
    let last = visual
        .debug_bounds("TAB-name@cy")
        .expect("the last tab draws");
    let drop_target = visual
        .debug_bounds("TABBAR-tabs")
        .expect("the bar's drop target draws");
    let button = visual
        .debug_bounds("NEW-tabs")
        .expect("the new button draws");
    assert!(
        button.origin.x > last.right(),
        "the new button is after the tabs: {button:?} vs {last:?}"
    );
    assert!(
        button.origin.x >= drop_target.right(),
        "the new button is outside the scrolling row: {button:?} vs {drop_target:?}"
    );
    let bar = app
        .update(|cx| shell::node_bounds(cx, "tabs"))
        .expect("the tabs container is drawn");
    assert!(
        button.right() > bar.right() - px(80.) && button.right() <= bar.right(),
        "the new button is at the bar's end: {button:?} in {bar:?}"
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert!(errors().is_empty(), "{:?}", errors());
}

// A click on a copy's close button — drawn while the pointer is over the
// tab — and a middle click on the tab both call `close_item` with the
// copy's key once. What the model does then is its own: it removes `bob`
// and `cy`, so their tabs go and their views drop, and keeps `ada`, so its
// tab stays. None of it is recorded or saved.
#[gpui::test]
fn the_close_button_and_a_middle_click_ask_the_model_to_close_the_item_once(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    fs::write(&layout_path, r#"{"version":1,"operations":[]}"#).unwrap();
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let (_host, model, views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let layout_on_disk = fs::read_to_string(&layout_path).unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

    // Hidden until the pointer is over the tab: resting in a corner, a
    // click on the button's spot does not reach it.
    let button = visual
        .debug_bounds("CLOSE-name@bob")
        .expect("bob's tab has a close button")
        .center();
    visual.simulate_mouse_move(point(px(850.), px(550.)), None, gpui::Modifiers::default());
    draw(window, &mut app);
    visual.simulate_click(button, gpui::Modifiers::default());
    assert!(
        app.update(|cx| model.read(cx).closed.is_empty()),
        "the button is hidden until the pointer is over the tab"
    );

    // Over the tab, a click on the button calls `close_item` once with the
    // copy's key; the model removes `bob`, so its tab goes and its view is
    // dropped.
    let bob = visual
        .debug_bounds("TAB-name@bob")
        .expect("bob's tab draws")
        .center();
    visual.simulate_mouse_move(bob, None, gpui::Modifiers::default());
    draw(window, &mut app);
    let button = visual
        .debug_bounds("CLOSE-name@bob")
        .expect("bob's close button draws on hover")
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    assert_eq!(
        app.update(|cx| model.read(cx).closed.clone()),
        [SharedString::from("bob")],
        "close_item is called once, with bob's key"
    );
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-name@bob").is_none(),
        "bob's tab is gone"
    );
    assert!(
        views.borrow()["name@bob"].upgrade().is_none(),
        "bob's view is dropped"
    );
    for tab in ["TAB-name@ada", "TAB-name@cy"] {
        assert!(visual.debug_bounds(tab).is_some(), "{tab} stays");
    }

    // A middle click on `cy`'s tab asks the same way, once.
    let cy = visual
        .debug_bounds("TAB-name@cy")
        .expect("cy's tab draws")
        .center();
    visual.simulate_mouse_down(cy, gpui::MouseButton::Middle, gpui::Modifiers::default());
    visual.simulate_mouse_up(cy, gpui::MouseButton::Middle, gpui::Modifiers::default());
    assert_eq!(
        app.update(|cx| model.read(cx).closed.clone()),
        [SharedString::from("bob"), SharedString::from("cy")],
        "close_item is called once, with cy's key"
    );
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-name@cy").is_none(),
        "cy's tab is gone"
    );
    assert!(
        views.borrow()["name@cy"].upgrade().is_none(),
        "cy's view is dropped"
    );

    // The model keeps `ada`: asked once, its tab and its view stay.
    let ada = visual
        .debug_bounds("TAB-name@ada")
        .expect("ada's tab draws")
        .center();
    visual.simulate_mouse_move(ada, None, gpui::Modifiers::default());
    draw(window, &mut app);
    let button = visual
        .debug_bounds("CLOSE-name@ada")
        .expect("ada's close button draws on hover")
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    assert_eq!(
        app.update(|cx| model.read(cx).closed.clone()),
        [
            SharedString::from("bob"),
            SharedString::from("cy"),
            SharedString::from("ada"),
        ],
        "close_item is called once, with ada's key"
    );
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-name@ada").is_some(),
        "ada's tab stays: the model kept it"
    );
    assert!(
        views.borrow()["name@ada"].upgrade().is_some(),
        "ada's view stays"
    );
    assert_eq!(
        selected(&mut app).as_deref(),
        Some("name@ada"),
        "the remaining tab is selected"
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded"
    );
    assert_eq!(
        fs::read_to_string(&layout_path).unwrap(),
        layout_on_disk,
        "nothing is saved"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// A click on the new button calls `new_item` once; the key the model adds
// gets a tab, which is the selected one. Nothing is recorded or saved.
#[gpui::test]
fn clicking_the_new_button_asks_the_model_for_an_item_and_selects_its_tab(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("layout.json");
    fs::write(&layout_path, r#"{"version":1,"operations":[]}"#).unwrap();
    let mut app = cx.new_app();
    app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
    let (_host, model, _views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let layout_on_disk = fs::read_to_string(&layout_path).unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    for (n, tab, card, node) in [
        (1usize, "TAB-name@new-1", "CARD-name@new-1", "name@new-1"),
        (2, "TAB-name@new-2", "CARD-name@new-2", "name@new-2"),
    ] {
        let button = visual
            .debug_bounds("NEW-tabs")
            .expect("the new button draws")
            .center();
        visual.simulate_click(button, gpui::Modifiers::default());
        assert_eq!(
            app.update(|cx| model.read(cx).added),
            n,
            "new_item is called once per click"
        );
        draw(window, &mut app);
        assert!(visual.debug_bounds(tab).is_some(), "{node}'s tab draws");
        assert_eq!(
            selected(&mut app).as_deref(),
            Some(node),
            "the new item's tab is selected"
        );
        assert!(
            visual.debug_bounds(card).is_some(),
            "the selected copy's view is shown"
        );
    }
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "nothing is recorded"
    );
    assert_eq!(
        fs::read_to_string(&layout_path).unwrap(),
        layout_on_disk,
        "nothing is saved"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// When the selected item leaves — closed by the model or dropped by a
// change of the items — the tabs container selects its first shown tab, as
// it does after a close.
#[gpui::test]
fn when_the_selected_item_leaves_the_first_shown_tab_is_selected(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, model, _views) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // `cy` is selected; a middle click on its tab asks the model, which
    // removes it: the first shown tab is selected.
    let cy = visual
        .debug_bounds("TAB-name@cy")
        .expect("cy's tab draws")
        .center();
    visual.simulate_click(cy, gpui::Modifiers::default());
    draw(window, &mut app);
    assert_eq!(selected(&mut app).as_deref(), Some("name@cy"));
    visual.simulate_mouse_down(cy, gpui::MouseButton::Middle, gpui::Modifiers::default());
    visual.simulate_mouse_up(cy, gpui::MouseButton::Middle, gpui::Modifiers::default());
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-name@cy").is_none(),
        "cy's tab is gone"
    );
    assert_eq!(
        selected(&mut app).as_deref(),
        Some("name@ada"),
        "closing the selected item selects the first shown tab"
    );
    // The model dropping the selected item does the same.
    let bob = visual
        .debug_bounds("TAB-name@bob")
        .expect("bob's tab draws")
        .center();
    visual.simulate_click(bob, gpui::Modifiers::default());
    draw(window, &mut app);
    assert_eq!(selected(&mut app).as_deref(), Some("name@bob"));
    app.update(|cx| {
        model.update(cx, |names, cx| {
            names.names.retain(|name| name.as_str() != "bob");
            cx.notify();
        });
    });
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-name@bob").is_none(),
        "bob's tab is gone"
    );
    assert_eq!(
        selected(&mut app).as_deref(),
        Some("name@ada"),
        "the selected item leaving selects the first shown tab"
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert!(errors().is_empty(), "{:?}", errors());
}

// `item_tabs` on a node that is not in the tree, or whose component is not
// `riso.tabs`, is refused with an ERROR and declares no item slot.
#[gpui::test]
fn item_tabs_on_a_missing_node_or_a_node_that_is_not_riso_tabs_is_refused(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _model, _views) = setup(&mut app);
    app.update(|cx| {
        host.item_tabs::<Names>(cx, "missing");
        host.place(cx, "row", "riso.row", "root", "children");
        host.item_tabs::<Names>(cx, "row");
        host.place(cx, "plain", "tour.card", "row", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let errors = errors();
    assert_eq!(errors.len(), 2, "{errors:?}");
    for node in ["missing", "row"] {
        assert!(
            errors.iter().any(|message| message.contains(node)),
            "missing the refusal of {node}: {errors:?}"
        );
    }
    assert!(
        visual.debug_bounds("CARD-plain").is_some(),
        "the row shows its child once, as before"
    );
    assert!(
        app.update(|cx| shell::item(cx, "plain@ada")).is_none(),
        "no item slot was declared on the row"
    );
    assert!(
        visual.debug_bounds("TAB-name@ada").is_some(),
        "the real item tabs are unaffected"
    );
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
}
