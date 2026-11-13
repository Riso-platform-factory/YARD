use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::{FocusDown, FocusLeft, FocusRight};

struct Card(String);

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.0))
            .size_full()
    }
}

fn draw(visual: &mut gpui::VisualTestContext) {
    visual.update(|window, cx| {
        // Resize and focus must invalidate the frame themselves.
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

#[gpui::test]
fn nested_split_focus_uses_resized_bounds(cx: &mut TestAppContext) {
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
        cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
            .unwrap()
    });
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    draw(&mut visual);
    // Exercise both growth and shrinkage after an initial draw.
    for (width, height) in [(1440., 900.), (900., 600.)] {
        visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
        draw(&mut visual);
        let b = visual.debug_bounds("CARD-b").expect("b has bounds");
        let c = visual.debug_bounds("CARD-c").expect("c has bounds");
        assert_eq!(b.size.width, gpui::px(width / 2.));
        assert_eq!(b.size.height, gpui::px(height / 2.));
        assert_eq!(b.bottom(), c.top());

        click(&mut visual, "CARD-a");
        visual.dispatch_action(FocusRight);
        draw(&mut visual);
        assert_eq!(focused(&mut visual).as_deref(), Some("c"));

        click(&mut visual, "CARD-b");
        assert_eq!(focused(&mut visual).as_deref(), Some("b"));
        visual.dispatch_action(FocusDown);
        draw(&mut visual);
        assert_eq!(focused(&mut visual).as_deref(), Some("c"));

        click(&mut visual, "CARD-b");
        assert_eq!(focused(&mut visual).as_deref(), Some("b"));
        visual.dispatch_action(FocusLeft);
        draw(&mut visual);
        assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    }
}
