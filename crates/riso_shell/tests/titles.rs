#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, Styled, TestAppContext, px};
use riso_layout::NodeId;
use riso_shell::dev as shell;
use serde_json::json;
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

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CardProps {
    title: String,
}

// A titled view whose title comes from its props.
struct Card {
    props: CardProps,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

impl shell::PropsView<CardProps> for Card {
    fn set_props(&mut self, props: CardProps, _: &mut gpui::Context<Self>) {
        self.props = props;
    }
}

impl shell::Titled for Card {
    fn title(&self, _: &gpui::App) -> gpui::SharedString {
        self.props.title.clone().into()
    }
}

// A titled view whose title is fixed when it is created.
struct TitledCard {
    title: gpui::SharedString,
}

impl gpui::Render for TitledCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

impl shell::Titled for TitledCard {
    fn title(&self, _: &gpui::App) -> gpui::SharedString {
        self.title.clone()
    }
}

// A titled view that also gives an icon.
struct IconedCard;

impl gpui::Render for IconedCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

impl shell::Titled for IconedCard {
    fn title(&self, _: &gpui::App) -> gpui::SharedString {
        "www-node".into()
    }
    fn icon(&self, _: &gpui::App) -> Option<riso_ui::IconName> {
        Some(riso_ui::IconName::Info)
    }
}

// A view that gives no title.
struct PlainCard;

impl gpui::Render for PlainCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
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
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}

// The width of the drawn tab a `TAB-<node>` selector records. A tab is as wide
// as another tab that draws the same text, which is what the checks compare.
fn tab_width(visual: &mut gpui::VisualTestContext, selector: &'static str) -> gpui::Pixels {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
        .size
        .width
}

fn setup(app: &mut TestAppContext) -> (shell::Extension, shell::Extension) {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let guest = shell::Extension::new(cx, "guest").unwrap();
        host.register(cx, "tour.simple", &[], move |node, _, _, cx| {
            let title = match node.as_str() {
                "alpha" => "alpha-one",
                "gamma-twin" => "gamma",
                "mismatch-twin" => "mismatch",
                other => panic!("unexpected tour.simple node {other}"),
            };
            cx.new(|_| TitledCard {
                title: title.into(),
            })
            .into()
        });
        host.register_title::<TitledCard>(cx, "tour.simple");
        host.register_with_props::<CardProps, Card>(
            cx,
            "tour.card",
            &[],
            move |props, _, _, _, _| Card { props },
        );
        host.register_title::<Card>(cx, "tour.card");
        host.register(cx, "tour.iconed", &[], move |_, _, _, cx| {
            cx.new(|_| IconedCard).into()
        });
        host.register_title::<IconedCard>(cx, "tour.iconed");
        // The extension bug the contract names: tour.wrong's views are
        // PlainCard, but its title is registered for Card.
        host.register(cx, "tour.wrong", &[], move |_, _, _, cx| {
            cx.new(|_| PlainCard).into()
        });
        host.register_title::<Card>(cx, "tour.wrong");
        host.register(cx, "tour.plain", &[], move |_, _, _, cx| {
            cx.new(|_| PlainCard).into()
        });
        guest.register(cx, "guest.card", &[], move |_, _, _, cx| {
            cx.new(|_| PlainCard).into()
        });
        host.place_root(cx, "tabs", "riso.tabs");
        host.place(cx, "alpha", "tour.simple", "tabs", "children");
        host.place_with_props(
            cx,
            "beta",
            "tour.card",
            "tabs",
            "children",
            json!({"title": "b-one"}),
        );
        host.place(cx, "gamma", "tour.plain", "tabs", "children");
        host.place(cx, "iconed", "tour.iconed", "tabs", "children");
        host.place(cx, "gamma-twin", "tour.simple", "tabs", "children");
        host.place(cx, "mismatch-twin", "tour.simple", "tabs", "children");
        host.place(cx, "alpha-one", "tour.plain", "tabs", "children");
        host.place(cx, "b-one", "tour.plain", "tabs", "children");
        host.place(cx, "b-two-and-then-some", "tour.plain", "tabs", "children");
        host.place(cx, "www-node", "tour.plain", "tabs", "children");
        host.place(cx, "early-one", "tour.plain", "tabs", "children");
        host.place(cx, "late-one", "tour.plain", "tabs", "children");
        (host, guest)
    })
}

#[gpui::test]
fn a_titled_nodes_tab_shows_the_title_as_wide_as_the_same_text_as_an_id(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _guest) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // alpha's view gives "alpha-one": the tab is as wide as the tab of the node
    // whose ID is that title, not of a node named alpha.
    assert_eq!(
        tab_width(&mut visual, "TAB-alpha"),
        tab_width(&mut visual, "TAB-alpha-one")
    );
    // beta's title comes through its props.
    assert_eq!(
        tab_width(&mut visual, "TAB-beta"),
        tab_width(&mut visual, "TAB-b-one")
    );
    assert_ne!(
        tab_width(&mut visual, "TAB-alpha"),
        tab_width(&mut visual, "TAB-beta")
    );
}

#[gpui::test]
fn an_untitled_nodes_tab_shows_its_node_id(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _guest) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // gamma has no title: it shows "gamma", as wide as gamma-twin whose view
    // gives the same text as a title.
    assert_eq!(
        tab_width(&mut visual, "TAB-gamma"),
        tab_width(&mut visual, "TAB-gamma-twin")
    );
    assert_ne!(
        tab_width(&mut visual, "TAB-gamma"),
        tab_width(&mut visual, "TAB-alpha-one")
    );
}

#[gpui::test]
fn a_title_change_is_shown_on_the_next_frame(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        tab_width(&mut visual, "TAB-beta"),
        tab_width(&mut visual, "TAB-b-one")
    );
    app.update(|cx| host.set_props(cx, "beta", json!({"title": "b-two-and-then-some"})));
    draw(window, &mut app);
    assert_eq!(
        tab_width(&mut visual, "TAB-beta"),
        tab_width(&mut visual, "TAB-b-two-and-then-some")
    );
}

#[gpui::test]
fn a_titled_tab_with_an_icon_shows_the_icon_and_the_title(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _guest) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let tab = visual.debug_bounds("TAB-iconed").expect("the tab draws");
    let icon = visual
        .debug_bounds("ICON-iconed")
        .expect("the tab draws its icon");
    // www-node has the same title text without an icon.
    assert!(
        tab.size.width > tab_width(&mut visual, "TAB-www-node"),
        "the icon adds width: {tab:?}"
    );
    assert!(
        tab.contains(&icon.center()),
        "the icon draws inside the tab: {icon:?} in {tab:?}"
    );
    assert!(
        icon.center().x < tab.center().x,
        "the icon is drawn before the title: {icon:?} in {tab:?}"
    );
    assert!(
        visual.debug_bounds("ICON-alpha").is_none(),
        "alpha's view gives no icon"
    );
}

#[gpui::test]
fn a_view_of_another_type_shows_the_node_id_and_logs_one_error(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = setup(&mut app);
    app.update(|cx| host.place(cx, "mismatch", "tour.wrong", "tabs", "children"));
    let window = open_and_draw(&mut app);
    draw(window, &mut app);
    draw(window, &mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    // mismatch shows "mismatch", its node ID: as wide as mismatch-twin, whose
    // view gives the same text as a title.
    assert_eq!(
        tab_width(&mut visual, "TAB-mismatch"),
        tab_width(&mut visual, "TAB-mismatch-twin")
    );
    let errors = errors();
    let wrong_view: Vec<&String> = errors
        .iter()
        .filter(|message| message.contains("view of node mismatch"))
        .collect();
    assert_eq!(
        wrong_view.len(),
        1,
        "one ERROR though the tab was drawn three times: {errors:?}"
    );
    assert_eq!(
        wrong_view[0].as_str(),
        "view of node mismatch is not the type that gives its title"
    );
}

#[gpui::test]
fn register_title_before_and_after_register_both_work(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = setup(&mut app);
    app.update(|cx| {
        // tour.early says so before registering, tour.late after.
        host.register_title::<TitledCard>(cx, "tour.early");
        host.register(cx, "tour.early", &[], |_, _, _, cx| {
            cx.new(|_| TitledCard {
                title: "early-one".into(),
            })
            .into()
        });
        host.register(cx, "tour.late", &[], |_, _, _, cx| {
            cx.new(|_| TitledCard {
                title: "late-one".into(),
            })
            .into()
        });
        host.register_title::<TitledCard>(cx, "tour.late");
        host.place(cx, "early", "tour.early", "tabs", "children");
        host.place(cx, "late", "tour.late", "tabs", "children");
    });
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        tab_width(&mut visual, "TAB-early"),
        tab_width(&mut visual, "TAB-early-one")
    );
    assert_eq!(
        tab_width(&mut visual, "TAB-late"),
        tab_width(&mut visual, "TAB-late-one")
    );
    assert!(errors().is_empty(), "no ERRORs: {:?}", errors());
}

#[gpui::test]
fn register_title_is_refused_for_another_extensions_component_an_invalid_id_and_a_second_time(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest) = setup(&mut app);
    app.update(|cx| {
        host.register_title::<TitledCard>(cx, "bad component");
        host.register_title::<TitledCard>(cx, "guest.card");
        // tour.card already gives titles from the setup.
        host.register_title::<Card>(cx, "tour.card");
    });
    let errors = errors();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(
        errors
            .iter()
            .any(|message| message.contains("bad component")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|message| message.contains("guest.card")),
        "{errors:?}"
    );
    assert!(
        errors.iter().any(|message| message.contains("tour.card")),
        "{errors:?}"
    );
    // The registrations that were not refused still work.
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    assert_eq!(
        tab_width(&mut visual, "TAB-alpha"),
        tab_width(&mut visual, "TAB-alpha-one")
    );
}

#[gpui::test]
fn selecting_and_dropping_a_titled_tab_works_on_its_node_id(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _guest) = setup(&mut app);
    let window = open_and_draw(&mut app);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
    let beta = visual
        .debug_bounds("TAB-beta")
        .expect("beta's tab draws")
        .center();
    visual.simulate_click(beta, Default::default());
    // The tab's ID is the node ID: clicking the tab selects beta, not a title.
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("beta"))
    );
    // Drag alpha's tab onto gamma's: alpha moves after gamma, as TAB-alpha.
    let start = visual
        .debug_bounds("TAB-alpha")
        .expect("alpha's tab draws")
        .center();
    visual.simulate_mouse_move(start, None, gpui::Modifiers::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    let nudge = gpui::point(start.x + px(5.), start.y);
    visual.simulate_mouse_move(nudge, gpui::MouseButton::Left, gpui::Modifiers::default());
    draw(window, &mut app);
    let target = visual
        .debug_bounds("TAB-gamma")
        .expect("gamma's tab draws")
        .center();
    let mid = gpui::point((start.x + target.x) * 0.5, (start.y + target.y) * 0.5);
    visual.simulate_mouse_move(mid, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    draw(window, &mut app);
    let alpha = visual
        .debug_bounds("TAB-alpha")
        .expect("alpha's tab still draws");
    let gamma = visual
        .debug_bounds("TAB-gamma")
        .expect("gamma's tab still draws");
    assert!(
        alpha.origin.x > gamma.origin.x,
        "alpha dropped after gamma: {alpha:?} vs {gamma:?}"
    );
    // The moved tab still shows its title.
    assert_eq!(alpha.size.width, tab_width(&mut visual, "TAB-alpha-one"));
    let center = alpha.center();
    visual.simulate_click(center, Default::default());
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs")),
        Some(id("alpha"))
    );
}
