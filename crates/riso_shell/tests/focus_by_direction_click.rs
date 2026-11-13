use gpui::{AppContext, InteractiveElement, IntoElement, ParentElement, Styled, TestAppContext};
use riso_shell::{FocusDown, FocusLeft};

struct Card(String);

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.0))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .child(self.0.clone())
    }
}

fn draw(visual: &mut gpui::VisualTestContext) {
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual.debug_bounds(selector).expect("card has bounds");
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| riso_shell::focused_node(window, cx).map(|node| node.to_string()))
}

fn check_click_direction(cx: &mut TestAppContext, down: bool) {
    cx.update(|cx| {
        riso_ui::init(cx);
        let ext = riso_shell::Extension::new(cx, "tour").unwrap();
        ext.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card(node.to_string())).into()
        });
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "a", "tour.card", "root", "children");
        ext.place(cx, "right", "riso.column", "root", "children");
        for node in ["b", "c"] {
            ext.place(cx, node, "tour.card", "right", "children");
        }
    });
    let window = cx.update(|cx| {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    gpui::point(gpui::px(0.), gpui::px(0.)),
                    gpui::size(gpui::px(900.), gpui::px(600.)),
                ))),
                ..Default::default()
            },
            riso_shell::new_shell,
        )
        .unwrap()
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    click(&mut visual, "CARD-b");
    draw(&mut visual);
    assert_eq!(focused(&mut visual).as_deref(), Some("b"));
    assert!(!visual.update(|_, cx| cx.has_active_drag()));
    let body = visual.update(|_, cx| riso_shell::node_bounds(cx, "body").unwrap());
    for (node, selector) in [("a", "CARD-a"), ("b", "CARD-b"), ("c", "CARD-c")] {
        let drawn = visual.debug_bounds(selector).expect("card is drawn");
        assert_eq!(drawn.size.width, gpui::px(450.));
        assert_eq!(
            drawn.size.height,
            body.size.height / if node == "a" { 1. } else { 2. }
        );
        let recorded = visual.update(|_, cx| riso_shell::node_bounds(cx, node));
        assert_eq!(
            recorded,
            Some(drawn),
            "{node}'s recorded bounds match its card"
        );
    }
    if down {
        visual.dispatch_action(FocusDown);
    } else {
        visual.dispatch_action(FocusLeft);
    }
    draw(&mut visual);
    assert_eq!(
        focused(&mut visual).as_deref(),
        Some(if down { "c" } else { "a" })
    );
}

#[gpui::test]
fn click_focused_b_moves_down(cx: &mut TestAppContext) {
    check_click_direction(cx, true);
}

#[gpui::test]
fn click_focused_b_moves_left(cx: &mut TestAppContext) {
    check_click_direction(cx, false);
}
