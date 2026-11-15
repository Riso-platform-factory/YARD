use gpui::{
    AppContext, InteractiveElement, IntoElement, ScrollDelta, ScrollEvent, Styled, TestAppContext,
    point, px,
};
use riso_shell as shell;
use std::sync::{Mutex, Once};

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

// The window is narrow: twenty `tNN` tabs cannot fit in its width.
const WINDOW_WIDTH: f32 = 480.;
const TABS: [&str; 20] = [
    "t01", "t02", "t03", "t04", "t05", "t06", "t07", "t08", "t09", "t10", "t11", "t12", "t13",
    "t14", "t15", "t16", "t17", "t18", "t19", "t20",
];
const SELECTORS: [&str; 20] = [
    "TAB-t01", "TAB-t02", "TAB-t03", "TAB-t04", "TAB-t05", "TAB-t06", "TAB-t07", "TAB-t08",
    "TAB-t09", "TAB-t10", "TAB-t11", "TAB-t12", "TAB-t13", "TAB-t14", "TAB-t15", "TAB-t16",
    "TAB-t17", "TAB-t18", "TAB-t19", "TAB-t20",
];

// A card fills the zone its container gives it, found as `CARD-<node>`.
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
    app.simulate_window_resize(window.into(), gpui::size(px(WINDOW_WIDTH), px(360.)));
    draw(window, app);
    window
}

// A tabs container over twenty cards, placed before the window opens so they
// are the default layout; the cards' tabs are `TAB-<node>`.
fn setup(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host.place_root(cx, "tabs", "riso.tabs");
        for node in TABS {
            host.place(cx, node, "tour.card", "tabs", "children");
        }
        host
    })
}

// The bar spans the window's width: a tab is in view whole when its bounds lie
// inside it.
fn in_view(tab: gpui::Bounds<gpui::Pixels>) -> bool {
    tab.origin.x >= px(-0.5) && tab.right() <= px(WINDOW_WIDTH + 0.5)
}

fn selected(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|_, cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

// The bounds of `node`'s tab, for a node of `TABS`.
fn tab_bounds(visual: &mut gpui::VisualTestContext, node: &str) -> gpui::Bounds<gpui::Pixels> {
    for (tab, selector) in TABS.into_iter().zip(SELECTORS) {
        if tab == node {
            return visual
                .debug_bounds(selector)
                .expect("the selected tab draws");
        }
    }
    panic!("no tab named {node}");
}

// The last `TAB-tNN` selector whose tab is in view whole, with a margin, after
// the first.
fn last_in_view(
    visual: &mut gpui::VisualTestContext,
) -> (&'static str, gpui::Bounds<gpui::Pixels>) {
    let mut found = None;
    for selector in SELECTORS.into_iter().skip(1) {
        if let Some(bounds) = visual.debug_bounds(selector)
            && bounds.origin.x >= px(1.)
            && bounds.right() <= px(WINDOW_WIDTH - 1.)
        {
            found = Some((selector, bounds));
        }
    }
    found.expect("a tab after the first is in view")
}

#[gpui::test]
fn selecting_the_last_tab_by_click_after_scrolling_scrolls_it_fully_into_view(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let _host = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // The bar shows the first tabs; the rest are past its end.
    let last = visual.debug_bounds("TAB-t20").expect("t20's tab draws");
    assert!(!in_view(last), "t20 starts past the bar's end: {last:?}");
    // Scroll the row until t20's leading edge shows, then click that edge.
    let over = visual
        .debug_bounds("TAB-t01")
        .expect("t01's tab draws")
        .center();
    let needed = px(WINDOW_WIDTH) - px(20.) - last.origin.x;
    visual.simulate_event(ScrollEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(needed, px(0.))),
        ..Default::default()
    });
    draw(window, &mut app);
    let sliver = visual.debug_bounds("TAB-t20").expect("t20's tab draws");
    assert!(
        sliver.origin.x < px(WINDOW_WIDTH) && sliver.right() > px(WINDOW_WIDTH),
        "t20 shows a leading edge after the scroll: {sliver:?}"
    );
    visual.simulate_click(
        point((sliver.origin.x + px(WINDOW_WIDTH)) / 2., sliver.center().y),
        gpui::Modifiers::default(),
    );
    draw(window, &mut app);
    assert_eq!(selected(&mut visual).as_deref(), Some("t20"));
    let last = visual.debug_bounds("TAB-t20").expect("t20's tab draws");
    assert!(
        in_view(last),
        "selecting t20 scrolled it into view whole: {last:?}"
    );
    assert!(
        last.right() >= px(WINDOW_WIDTH - 0.5),
        "scrolled just enough: t20 is whole at the bar's end: {last:?}"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

#[gpui::test]
fn select_next_tab_from_the_first_to_the_last_tab_scrolls_it_fully_into_view(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let _host = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let last = visual.debug_bounds("TAB-t20").expect("t20's tab draws");
    assert!(!in_view(last), "t20 starts past the bar's end: {last:?}");
    assert_eq!(selected(&mut visual).as_deref(), Some("t01"));
    // Focus a card in the container so the action reaches it, then step to t20.
    click(&mut visual, "CARD-t01");
    draw(window, &mut app);
    for _ in 1..TABS.len() {
        visual.dispatch_action(shell::SelectNextTab);
    }
    draw(window, &mut app);
    assert_eq!(selected(&mut visual).as_deref(), Some("t20"));
    let last = visual.debug_bounds("TAB-t20").expect("t20's tab draws");
    assert!(
        in_view(last),
        "SelectNextTab scrolled t20 into view whole: {last:?}"
    );
    assert!(
        last.right() >= px(WINDOW_WIDTH - 0.5),
        "scrolled just enough: t20 is whole at the bar's end: {last:?}"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

#[gpui::test]
fn selecting_a_tab_that_is_already_in_view_does_not_scroll(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let _host = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let first = visual.debug_bounds("TAB-t01").expect("t01's tab draws");
    let (selector, before) = last_in_view(&mut visual);
    click(&mut visual, selector);
    draw(window, &mut app);
    assert_eq!(
        selected(&mut visual).as_deref(),
        Some(selector.strip_prefix("TAB-").unwrap())
    );
    assert_eq!(
        visual.debug_bounds("TAB-t01").expect("t01's tab draws"),
        first,
        "the row did not move"
    );
    assert_eq!(
        visual
            .debug_bounds(selector)
            .expect("the selected tab draws"),
        before,
        "the selected tab did not move"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

#[gpui::test]
fn closing_the_selected_last_tab_scrolls_the_newly_selected_tab_into_view(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let _host = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // Select t20, stepping to it from the first tab.
    click(&mut visual, "CARD-t01");
    draw(window, &mut app);
    for _ in 1..TABS.len() {
        visual.dispatch_action(shell::SelectNextTab);
    }
    draw(window, &mut app);
    assert_eq!(selected(&mut visual).as_deref(), Some("t20"));
    let t20 = visual
        .debug_bounds("TAB-t20")
        .expect("t20's tab draws")
        .center();
    // The button is drawn only while the pointer is over the tab.
    visual.simulate_mouse_move(t20, None, gpui::Modifiers::default());
    draw(window, &mut app);
    let button = visual
        .debug_bounds("CLOSE-t20")
        .expect("t20's close button is drawn")
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    draw(window, &mut app);
    assert!(
        visual.debug_bounds("TAB-t20").is_none(),
        "t20's tab is gone"
    );
    let selected = selected(&mut visual).expect("a tab is selected");
    let bounds = tab_bounds(&mut visual, &selected);
    assert!(
        in_view(bounds),
        "the newly selected {selected} is scrolled into view: {bounds:?}"
    );
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        1,
        "the close is the only thing recorded"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

#[gpui::test]
fn the_scroll_position_is_view_state_and_is_not_recorded(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let _host = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // Wheel the row itself.
    let over = visual
        .debug_bounds("TAB-t01")
        .expect("t01's tab draws")
        .center();
    visual.simulate_event(ScrollEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(-160.), px(0.))),
        ..Default::default()
    });
    draw(window, &mut app);
    // Select to the last tab: the row scrolls it into view; both are view
    // state, nothing is recorded.
    click(&mut visual, "CARD-t01");
    draw(window, &mut app);
    for _ in 1..TABS.len() {
        visual.dispatch_action(shell::SelectNextTab);
    }
    draw(window, &mut app);
    assert_eq!(selected(&mut visual).as_deref(), Some("t20"));
    assert_eq!(
        app.update(|cx| shell::user_layer_len(cx)),
        0,
        "scrolling and selecting record nothing"
    );
    assert!(errors().is_empty(), "{:?}", errors());
}
