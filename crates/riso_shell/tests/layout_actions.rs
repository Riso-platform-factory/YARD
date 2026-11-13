#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::dev::{Hide, SelectNextTab, SelectPreviousTab, ToggleZoom};

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

fn draw(window: gpui::WindowHandle<riso_shell::dev::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Opens a window over the layout that `place` builds.
fn open_cards_app(
    cx: &mut TestAppContext,
    place: impl FnOnce(&mut gpui::App, &riso_shell::dev::Extension),
) -> (
    riso_shell::dev::Extension,
    gpui::WindowHandle<riso_shell::dev::Shell>,
) {
    let ext = cx.update(|cx| {
        let ext = riso_shell::dev::Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        place(cx, &ext);
        ext
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    (ext, window)
}

fn row_of_three(cx: &mut gpui::App, ext: &riso_shell::dev::Extension) {
    ext.place_root(cx, "root", "riso.row");
    for node in ["a", "b", "c"] {
        ext.place(cx, node, "tour.card", "root", "children");
    }
}

/// The node the dev shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual
        .update(|window, cx| riso_shell::dev::focused_node(window, cx).map(|node| node.to_string()))
}

/// The node shown by the `tabs` container, if any.
fn selected(visual: &mut gpui::VisualTestContext, tabs: &str) -> Option<String> {
    visual.update(|_, cx| riso_shell::dev::selected_tab(cx, tabs).map(|node| node.to_string()))
}

/// Clicks the center of the element with `selector` and returns its bounds.
fn click(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Bounds<gpui::Pixels> {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
    bounds
}

#[gpui::test]
fn toggle_zoom_on_a_focused_card_zooms_it_then_zooms_out(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(ToggleZoom);
    draw(window, cx);
    assert_eq!(
        cx.update(|cx| riso_shell::dev::zoomed(cx)).as_deref(),
        Some("b")
    );
    visual.dispatch_action(ToggleZoom);
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::dev::zoomed(cx)), None);
}

#[gpui::test]
fn hide_on_a_focused_card_hides_it_and_leaves_no_node_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(Hide);
    draw(window, cx);
    assert!(cx.update(|cx| riso_shell::dev::is_hidden(cx, "b")));
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn next_and_previous_tab_cycle_through_the_tabs_container(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        for node in ["a", "b", "c"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // After the first selection the focus is on the container, which cycles on.
    for expected in ["b", "c", "a"] {
        visual.dispatch_action(SelectNextTab);
        draw(window, cx);
        assert_eq!(selected(&mut visual, "tabs").as_deref(), Some(expected));
        assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    }
    visual.dispatch_action(SelectPreviousTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("c"));
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
}

#[gpui::test]
fn a_card_in_a_row_inside_tabs_selects_the_next_tab(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        ext.place(cx, "mid", "riso.row", "tabs", "children");
        for node in ["x", "y"] {
            ext.place(cx, node, "tour.card", "mid", "children");
        }
        for node in ["a", "b"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-x");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("x"));
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("a"));
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
}

#[gpui::test]
fn actions_change_nothing_when_no_node_is_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        ext.place(cx, "right", "riso.tabs", "root", "children");
        for node in ["b", "c"] {
            ext.place(cx, node, "tour.card", "right", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focused(&mut visual), None);
    visual.dispatch_action(ToggleZoom);
    visual.dispatch_action(Hide);
    visual.dispatch_action(SelectNextTab);
    visual.dispatch_action(SelectPreviousTab);
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::dev::zoomed(cx)), None);
    assert!(!cx.update(|cx| riso_shell::dev::is_hidden(cx, "a")));
    assert!(!cx.update(|cx| riso_shell::dev::is_hidden(cx, "b")));
    assert_eq!(selected(&mut visual, "right").as_deref(), Some("b"));
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn toggle_zoom_on_the_focused_root_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "tour.card");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-root");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("root"));
    visual.dispatch_action(ToggleZoom);
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::dev::zoomed(cx)), None);
    assert_eq!(focused(&mut visual).as_deref(), Some("root"));
}

#[gpui::test]
fn hide_on_the_focused_root_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "tour.card");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-root");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("root"));
    visual.dispatch_action(Hide);
    draw(window, cx);
    assert!(!cx.update(|cx| riso_shell::dev::is_hidden(cx, "root")));
    assert_eq!(focused(&mut visual).as_deref(), Some("root"));
}

#[gpui::test]
fn select_tab_on_a_card_with_no_tabs_above_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(SelectNextTab);
    visual.dispatch_action(SelectPreviousTab);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
}

#[gpui::test]
fn a_zoom_outside_the_tabs_stays_when_a_tab_is_selected(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        ext.place(cx, "tabs", "riso.tabs", "root", "children");
        for node in ["b", "c"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    cx.update(|cx| ext.set_zoomed(cx, "tabs"));
    draw(window, cx);
    assert_eq!(
        cx.update(|cx| riso_shell::dev::zoomed(cx)).as_deref(),
        Some("tabs")
    );
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("c"));
    assert_eq!(
        cx.update(|cx| riso_shell::dev::zoomed(cx)).as_deref(),
        Some("tabs")
    );
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
}

#[gpui::test]
fn a_tabs_container_with_no_shown_tab_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        for node in ["a", "b"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    cx.update(|cx| {
        ext.set_hidden(cx, "a", true);
        ext.set_hidden(cx, "b", true);
    });
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs"), None);
    click(&mut visual, "TABBAR-tabs");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs"), None);
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
}

#[gpui::test]
fn an_action_right_after_hide_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(Hide);
    visual.dispatch_action(ToggleZoom);
    draw(window, cx);
    assert!(cx.update(|cx| riso_shell::dev::is_hidden(cx, "b")));
    assert_eq!(cx.update(|cx| riso_shell::dev::zoomed(cx)), None);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn a_tabs_container_with_one_tab_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        ext.place(cx, "a", "tour.card", "tabs", "children");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let card = click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("a"));
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // The bar sits just above the selected card; its middle is outside every tab.
    visual.simulate_click(
        gpui::point(card.center().x, card.origin.y - gpui::px(1.)),
        gpui::Modifiers::default(),
    );
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    visual.dispatch_action(SelectPreviousTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("a"));
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
}
