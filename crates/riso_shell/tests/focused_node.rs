use gpui::{AppContext, InteractiveElement, IntoElement, ParentElement, Styled, TestAppContext};
use std::{cell::RefCell, rc::Rc};

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

/// A card whose view holds a control with its own focus handle.
struct ControlCard {
    node: String,
    control: gpui::FocusHandle,
}

impl gpui::Render for ControlCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
            .child(
                gpui::div()
                    .debug_selector(|| format!("CONTROL-{}", self.node))
                    .track_focus(&self.control)
                    .size_full(),
            )
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

/// Opens a window over the `tour.card` layout that `place` builds.
fn open_cards_app(
    cx: &mut TestAppContext,
    place: impl FnOnce(&mut gpui::App, &riso_shell::Extension),
) -> (riso_shell::Extension, gpui::WindowHandle<riso_shell::Shell>) {
    let ext = cx.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
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
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    (ext, window)
}

fn row_of_three(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    ext.place_root(cx, "root", "riso.row");
    for node in ["a", "b", "c"] {
        ext.place(cx, node, "tour.card", "root", "children");
    }
}

/// The node the shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string()))
}

/// `(is_focused, contains_focused)` for `node`, or `None` when it has no handle.
fn focus_flags(visual: &mut gpui::VisualTestContext, node: &str) -> Option<(bool, bool)> {
    visual.update(|window, cx| {
        riso_shell::focus_handle(cx, node).map(|handle| {
            (
                handle.is_focused(window),
                handle.contains_focused(window, cx),
            )
        })
    })
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
fn no_node_is_focused_when_the_window_opens(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn a_click_focuses_a_card_in_a_row(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    assert_eq!(focus_flags(&mut visual, "b"), Some((true, true)));
    click(&mut visual, "CARD-c");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    assert_eq!(focus_flags(&mut visual, "b"), Some((false, false)));
}

#[gpui::test]
fn containers_contain_the_focus_without_having_it(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        ext.place(cx, "right", "riso.column", "root", "children");
        for node in ["b", "c"] {
            ext.place(cx, node, "tour.card", "right", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-c");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    assert_eq!(focus_flags(&mut visual, "c"), Some((true, true)));
    assert_eq!(focus_flags(&mut visual, "right"), Some((false, true)));
    assert_eq!(focus_flags(&mut visual, "root"), Some((false, true)));
    assert_eq!(focus_flags(&mut visual, "a"), Some((false, false)));
}

#[gpui::test]
fn a_click_on_the_tab_bar_focuses_the_tabs_node(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        for node in ["a", "b"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let card = click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    assert_eq!(focus_flags(&mut visual, "tabs"), Some((false, true)));
    // The bar sits just above the selected card; its middle is outside every tab.
    visual.simulate_click(
        gpui::point(card.center().x, card.origin.y - gpui::px(1.)),
        gpui::Modifiers::default(),
    );
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    assert_eq!(focus_flags(&mut visual, "a"), Some((false, false)));
}

#[gpui::test]
fn moving_the_focused_node_to_an_unselected_tab_leaves_no_node_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "left", "riso.tabs", "root", "children");
        ext.place(cx, "right", "riso.tabs", "root", "children");
        for (node, parent) in [("a", "left"), ("b", "left"), ("c", "right")] {
            ext.place(cx, node, "tour.card", parent, "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // `a` keeps its view and stays shown, but `right` keeps `c` selected.
    cx.update(|cx| ext.move_node(cx, "a", "right", "children", usize::MAX));
    draw(window, cx);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn hiding_the_focused_node_leaves_no_node_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    cx.update(|cx| ext.set_hidden(cx, "b", true));
    draw(window, cx);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn removing_the_focused_node_leaves_no_node_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    cx.update(|cx| ext.remove(cx, "b"));
    draw(window, cx);
    assert_eq!(focused(&mut visual), None);
    assert_eq!(focus_flags(&mut visual, "b"), None);
}

#[gpui::test]
fn focus_handle_is_none_for_a_node_not_in_the_tree(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (_, window) = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.card", "root", "children");
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focus_flags(&mut visual, "nope"), None);
    assert_eq!(focus_flags(&mut visual, "a"), Some((false, false)));
}

#[gpui::test]
fn focus_handle_is_none_for_a_hidden_node(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (ext, window) = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focus_flags(&mut visual, "b"), Some((false, false)));
    cx.update(|cx| ext.set_hidden(cx, "b", true));
    draw(window, cx);
    assert_eq!(focus_flags(&mut visual, "b"), None);
}

#[gpui::test]
fn a_control_inside_a_card_keeps_the_focus(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let control: Rc<RefCell<Option<gpui::FocusHandle>>> = Rc::new(RefCell::new(None));
    let captured = control.clone();
    let (_, window) = open_cards_app(cx, move |cx, ext| {
        ext.register(cx, "tour.control", &[], move |node, _, _, cx| {
            let captured = captured.clone();
            cx.new(|cx| {
                let handle = cx.focus_handle();
                *captured.borrow_mut() = Some(handle.clone());
                ControlCard {
                    node: node.to_string(),
                    control: handle,
                }
            })
            .into()
        });
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.control", "root", "children");
    });
    let control = control.borrow().clone().unwrap();
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CONTROL-a");
    draw(window, cx);
    draw(window, cx);
    visual.update(|window, cx| {
        assert!(control.is_focused(window));
        assert_eq!(window.focused(cx).as_ref(), Some(&control));
    });
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    assert_eq!(focus_flags(&mut visual, "a"), Some((false, true)));
}
