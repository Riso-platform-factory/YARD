#![cfg(target_os = "macos")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::{Hide, SelectNextTab, SelectPreviousTab, ToggleZoom};

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

/// Binds the default keys, then opens a window over the layout that `place` builds.
fn open_cards_app(
    cx: &mut TestAppContext,
    place: impl FnOnce(&mut gpui::App, &riso_shell::Extension),
) -> gpui::WindowHandle<riso_shell::Shell> {
    cx.update(|cx| {
        // The startup binds the default keys in `app-init`, before extensions run.
        riso_shell::bind_default_keys(cx);
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

/// The node the dev shell says has the focus, if any.
fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string()))
}

/// The node shown by the `tabs` container, if any.
fn selected(visual: &mut gpui::VisualTestContext, tabs: &str) -> Option<String> {
    visual.update(|_, cx| riso_shell::selected_tab(cx, tabs).map(|node| node.to_string()))
}

/// Clicks the center of the element with `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

#[gpui::test]
fn shift_escape_zooms_the_focused_card_and_zooms_out(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-b");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    visual.simulate_keystrokes("shift-escape");
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::zoomed(cx)).as_deref(), Some("b"));
    visual.simulate_keystrokes("shift-escape");
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::zoomed(cx)), None);
}

#[gpui::test]
fn the_tab_keys_select_the_next_and_previous_tab(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, |cx, ext| {
        ext.place_root(cx, "tabs", "riso.tabs");
        for node in ["a", "b", "c"] {
            ext.place(cx, node, "tour.card", "tabs", "children");
        }
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    click(&mut visual, "CARD-a");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    for (keystrokes, expected) in [
        ("cmd-}", "b"),
        ("alt-cmd-right", "c"),
        ("cmd-{", "b"),
        ("alt-cmd-left", "a"),
    ] {
        visual.simulate_keystrokes(keystrokes);
        draw(window, cx);
        assert_eq!(selected(&mut visual, "tabs").as_deref(), Some(expected));
        assert_eq!(focused(&mut visual).as_deref(), Some("tabs"));
    }
}

#[gpui::test]
fn shift_escape_with_no_node_focused_changes_nothing(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let window = open_cards_app(cx, row_of_three);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    assert_eq!(focused(&mut visual), None);
    visual.simulate_keystrokes("shift-escape");
    draw(window, cx);
    assert_eq!(cx.update(|cx| riso_shell::zoomed(cx)), None);
    assert_eq!(focused(&mut visual), None);
}

#[gpui::test]
fn bind_default_keys_binds_the_macos_keys_of_each_action_in_the_shell_context(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        riso_shell::bind_default_keys(cx);
        let os = cx.platform_info().os;
        let shell = gpui::KeyBindingContextPredicate::parse("Shell").unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let table: [(&dyn gpui::Action, &[&str]); 4] = [
            (&ToggleZoom, &["shift-escape"]),
            (&SelectNextTab, &["cmd-}", "alt-cmd-right"]),
            (&SelectPreviousTab, &["cmd-{", "alt-cmd-left"]),
            (&Hide, &[]),
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
        assert_eq!(keymap.bindings().len(), 5, "every bound key");
    });
}
