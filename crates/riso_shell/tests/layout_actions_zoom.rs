use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::{SelectNextTab, SelectPreviousTab, ToggleZoom};

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

#[gpui::test]
fn selecting_adjacent_tabs_from_zoomed_nodes_zooms_out_and_focuses_tabs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        riso_ui::init(cx);
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        ext.place_root(cx, "tabs", "riso.tabs");
        ext.place(cx, "row", "riso.row", "tabs", "children");
        ext.place(cx, "nested", "tour.card", "row", "children");
        ext.place(cx, "other", "tour.card", "tabs", "children");
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    draw(window, cx);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    for (card, expected, next) in [("nested", "other", true), ("other", "row", false)] {
        let selector = if next { "CARD-nested" } else { "CARD-other" };
        let bounds = visual
            .debug_bounds(selector)
            .expect("selected card is drawn");
        visual.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(window, cx);
        visual.dispatch_action(ToggleZoom);
        draw(window, cx);
        assert_eq!(
            cx.update(|cx| riso_shell::zoomed(cx)).as_deref(),
            Some(card)
        );
        if next {
            visual.dispatch_action(SelectNextTab);
        } else {
            visual.dispatch_action(SelectPreviousTab);
        }
        draw(window, cx);
        assert_eq!(cx.update(|cx| riso_shell::zoomed(cx)), None);
        assert_eq!(
            cx.update(|cx| riso_shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
                .as_deref(),
            Some(expected)
        );
        assert_eq!(
            visual
                .update(
                    |window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string())
                )
                .as_deref(),
            Some("tabs")
        );
        let selected = if next { "CARD-other" } else { "CARD-nested" };
        assert!(visual.debug_bounds(selected).is_some(), "new tab is drawn");
    }
}
