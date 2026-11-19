use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use yard_shell as shell;

// The window is narrow: twenty `tNN` tabs cannot fit in its width.
const WINDOW_WIDTH: f32 = 480.;

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

fn in_view(tab: gpui::Bounds<gpui::Pixels>) -> bool {
    tab.origin.x >= px(-0.5) && tab.right() <= px(WINDOW_WIDTH + 0.5)
}

// Moving the selected tab to another place in its own bar keeps it selected,
// and scrolls its new place into view.
#[gpui::test]
fn moving_the_selected_tab_within_its_bar_scrolls_its_new_place_into_view(cx: &mut TestAppContext) {
    let host = cx.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host.place_root(cx, "tabs", "yard.tabs");
        for index in 1..=20 {
            host.place(cx, &format!("t{index:02}"), "tour.card", "tabs", "children");
        }
        host
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(WINDOW_WIDTH), px(360.)));
    draw(window, cx);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let selected = |cx: &mut TestAppContext| {
        cx.update(|cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
    };
    assert_eq!(selected(cx).as_deref(), Some("t01"));
    assert!(in_view(visual.debug_bounds("TAB-t01").unwrap()));

    // To the bar's end, as a drop on the bar's end does.
    cx.update(|cx| host.move_node(cx, "t01", "tabs", "children", usize::MAX));
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("t01"));
    let moved = visual.debug_bounds("TAB-t01").expect("t01's tab draws");
    assert!(
        in_view(moved),
        "the selected tab's new place at the end is scrolled into view: {moved:?}"
    );
    assert!(
        !in_view(visual.debug_bounds("TAB-t02").unwrap()),
        "the row moved to the end"
    );

    // And back to the start.
    cx.update(|cx| host.move_node(cx, "t01", "tabs", "children", 0));
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("t01"));
    let moved = visual.debug_bounds("TAB-t01").expect("t01's tab draws");
    assert!(
        in_view(moved),
        "the selected tab's new place at the start is scrolled into view: {moved:?}"
    );
}
