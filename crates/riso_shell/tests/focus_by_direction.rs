use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::{FocusDown, FocusLeft, FocusRight, FocusUp, SelectNextTab};

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

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, cx: &mut TestAppContext) {
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
    place: impl FnOnce(&mut gpui::App, &riso_shell::Extension),
) -> gpui::WindowHandle<riso_shell::Shell> {
    cx.update(|cx| {
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        place(cx, &ext);
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    window
}

fn row_of_three(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    ext.place_root(cx, "root", "riso.row");
    for node in ["a", "b", "c"] {
        ext.place(cx, node, "tour.card", "root", "children");
    }
}

/// A row of `a` and a column `right` of `b` above `c`.
fn nested_split(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    ext.place_root(cx, "root", "riso.row");
    ext.place(cx, "a", "tour.card", "root", "children");
    ext.place(cx, "right", "riso.column", "root", "children");
    for node in ["b", "c"] {
        ext.place(cx, node, "tour.card", "right", "children");
    }
}

/// A card `x` beside a `tabs` container of `a` and `b`.
fn card_and_tabs(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    ext.place_root(cx, "root", "riso.row");
    ext.place(cx, "x", "tour.card", "root", "children");
    ext.place(cx, "tabs", "riso.tabs", "root", "children");
    for node in ["a", "b"] {
        ext.place(cx, node, "tour.card", "tabs", "children");
    }
}

/// A card `top` above a `tabs` container of `x` and `y`.
fn card_above_tabs(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    ext.place_root(cx, "root", "riso.column");
    ext.place(cx, "top", "tour.card", "root", "children");
    ext.place(cx, "tabs", "riso.tabs", "root", "children");
    for node in ["x", "y"] {
        ext.place(cx, node, "tour.card", "tabs", "children");
    }
}

/// `nested_split` with `right` zoomed, so `a` is not drawn.
fn nested_split_zoomed(cx: &mut gpui::App, ext: &riso_shell::Extension) {
    nested_split(cx, ext);
    ext.set_zoomed(cx, "right");
}

/// The node the dev shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string()))
}

/// The node shown by the `tabs` container, if any.
fn selected(visual: &mut gpui::VisualTestContext, tabs: &str) -> Option<String> {
    visual.update(|_, cx| riso_shell::selected_tab(cx, tabs).map(|node| node.to_string()))
}

/// A node's bounds in the last draw, if it was drawn.
fn node_bounds(
    visual: &mut gpui::VisualTestContext,
    node: &str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    visual.update(|_, cx| riso_shell::node_bounds(cx, node))
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

/// Which of `b` and `c` holds the point 4px past the middle of `a`'s right edge.
fn target_right_of_a(visual: &mut gpui::VisualTestContext) -> &'static str {
    let a = visual
        .debug_bounds("CARD-a")
        .unwrap_or_else(|| panic!("CARD-a has bounds"));
    let probe = gpui::point(a.right() + gpui::px(4.), a.center().y);
    let b = visual
        .debug_bounds("CARD-b")
        .unwrap_or_else(|| panic!("CARD-b has bounds"));
    let c = visual
        .debug_bounds("CARD-c")
        .unwrap_or_else(|| panic!("CARD-c has bounds"));
    match (b.contains(&probe), c.contains(&probe)) {
        (true, false) => "b",
        (false, true) => "c",
        _ => panic!("{probe:?} sits in exactly one of b and c"),
    }
}

#[gpui::test]
fn left_and_right_walk_a_row_and_stop_at_its_edges(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(FocusLeft);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // `a` is the leftmost card: nothing is on its left.
    visual.dispatch_action(FocusLeft);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    // `c` is the rightmost card: nothing is on its right.
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
}

#[gpui::test]
fn moves_cross_into_a_column_and_walk_it_up_and_down(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, nested_split);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    // The point 4px past the middle of `a`'s right edge lands on `b` or `c`.
    let expected = target_right_of_a(&mut visual);
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some(expected));
    click(&mut visual, "CARD-b");
    draw(window, cx);
    visual.dispatch_action(FocusDown);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    visual.dispatch_action(FocusUp);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(FocusDown);
    draw(window, cx);
    visual.dispatch_action(FocusLeft);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
}

#[gpui::test]
fn a_move_into_tabs_targets_only_the_selected_tab_card(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, card_and_tabs);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-x");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("x"));
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("a"));
    // `b` is not shown: the only leaf inside the tabs is the selected card.
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("b"));
    click(&mut visual, "CARD-x");
    draw(window, cx);
    visual.dispatch_action(FocusRight);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
}

#[gpui::test]
fn a_move_crosses_a_tab_bar_into_and_out_of_a_tabs_container(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, card_above_tabs);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("x"));
    click(&mut visual, "CARD-top");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("top"));
    // The probe lands on the tab bar and moves on to the selected card `x`.
    visual.dispatch_action(FocusDown);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("x"));
    // Back up, the probe crosses the bar again to `top`.
    visual.dispatch_action(FocusUp);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("top"));
}

#[gpui::test]
fn inside_a_zoomed_column_a_move_stays_inside_it(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, nested_split_zoomed);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    // `a` is outside the zoomed subtree: it is not drawn.
    assert!(visual.debug_bounds("CARD-a").is_none());
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.dispatch_action(FocusDown);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    // `a` is not drawn, so it is never a candidate: the move is refused.
    visual.dispatch_action(FocusLeft);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
}

#[gpui::test]
fn a_move_starts_from_the_bounds_of_a_focused_tabs_container(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, card_and_tabs);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "TABBAR-tabs");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    visual.dispatch_action(FocusLeft);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("x"));
}

#[gpui::test]
fn the_moves_change_nothing_when_no_node_is_focused(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focused(&mut visual), None);
    visual.dispatch_action(FocusLeft);
    visual.dispatch_action(FocusRight);
    visual.dispatch_action(FocusUp);
    visual.dispatch_action(FocusDown);
    draw(window, cx);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn node_bounds_is_none_for_a_node_not_in_the_last_draw(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, card_and_tabs);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("a"));
    // `b` is an unselected tab: it was never drawn.
    assert!(node_bounds(&mut visual, "nope").is_none());
    assert!(node_bounds(&mut visual, "b").is_none());
    assert!(node_bounds(&mut visual, "a").is_some());
    click(&mut visual, "CARD-a");
    draw(window, cx);
    visual.dispatch_action(SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(&mut visual, "tabs").as_deref(), Some("b"));
    // `a`'s bounds of an earlier draw are kept, but `a` is not in the last draw.
    assert!(node_bounds(&mut visual, "a").is_none());
    assert!(node_bounds(&mut visual, "b").is_some());
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn the_direction_keys_move_the_focus(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    // The startup binds the default keys in `app-init`, before extensions run.
    cx.update(riso_shell::bind_default_keys);
    let window = open_cards_app(cx, nested_split);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    let expected = target_right_of_a(&mut visual);
    visual.simulate_keystrokes("cmd-k cmd-right");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some(expected));
    let (other, keystrokes) = match expected {
        "b" => ("c", "cmd-k cmd-down"),
        _ => ("b", "cmd-k cmd-up"),
    };
    visual.simulate_keystrokes(keystrokes);
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some(other));
    visual.simulate_keystrokes("cmd-k cmd-left");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn the_up_and_down_keys_move_the_focus(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    cx.update(riso_shell::bind_default_keys);
    let window = open_cards_app(cx, nested_split);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.simulate_keystrokes("cmd-k cmd-down");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("c"));
    visual.simulate_keystrokes("cmd-k cmd-up");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn bind_default_keys_binds_the_macos_direction_keys_in_the_shell_context(cx: &mut TestAppContext) {
    cx.update(|cx| {
        riso_shell::bind_default_keys(cx);
        let os = cx.platform_info().os;
        let shell = gpui::KeyBindingContextPredicate::parse("Shell").unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let table: [(&dyn gpui::Action, &[&str]); 4] = [
            (&FocusLeft, &["cmd-k cmd-left"]),
            (&FocusRight, &["cmd-k cmd-right"]),
            (&FocusUp, &["cmd-k cmd-up"]),
            (&FocusDown, &["cmd-k cmd-down"]),
        ];
        for (action, expected) in table {
            let bindings: Vec<_> = keymap.bindings_for_action(action).collect();
            let mut keys: Vec<String> = bindings
                .iter()
                .map(|binding| {
                    binding
                        .keystrokes()
                        .iter()
                        .map(|keystroke| keystroke.unparse(os))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            keys.sort();
            let mut expected = expected.to_vec();
            expected.sort_unstable();
            assert_eq!(keys, expected, "keys bound for {}", action.name());
            for binding in &bindings {
                assert_eq!(
                    binding.predicate().as_deref(),
                    Some(&shell),
                    "context of a {} binding",
                    action.name()
                );
            }
        }
    });
}
