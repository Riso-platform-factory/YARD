#![cfg(feature = "dev")]

use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, px};
use riso_shell::dev as shell;

#[gpui::test]
fn action_buttons_keep_the_layout_focus(cx: &mut TestAppContext) {
    // Select the real extension case before GPUI starts threads in a child process.
    if std::env::var("LAYOUT_TOUR_ACTIONS_TEST_CHILD").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "action_buttons_keep_the_layout_focus",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "actions")
            .env("LAYOUT_TOUR_ACTIONS_TEST_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    cx.update(|cx| {
        riso_ui::init(cx);
        let ext = shell::Extension::new(cx, "layout_tour").unwrap();
        layout_tour::init(cx, &ext);
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(1200.), px(600.)));
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for button in [
        "BUTTON-zoom",
        "BUTTON-hide",
        "BUTTON-next tab",
        "BUTTON-previous tab",
    ] {
        click(&mut visual, button);
        draw(window, cx);
        assert_eq!(focused(&mut visual), None);
    }
    assert_eq!(cx.update(|cx| shell::zoomed(cx)), None);
    assert!(!cx.update(|cx| shell::is_hidden(cx, "a")));
    assert_eq!(selected(cx).as_deref(), Some("b"));

    click(&mut visual, "CARD-a");
    draw(window, cx);
    click(&mut visual, "BUTTON-next tab");
    draw(window, cx);
    assert_eq!(focused(&mut visual).as_deref(), Some("a"));
    assert_eq!(selected(cx).as_deref(), Some("b"));
    click(&mut visual, "BUTTON-zoom");
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));
    assert_eq!(visual.debug_bounds("CARD-a").unwrap().size.width, px(1200.));
    // The controls are outside the zoomed card. Its existing zoom-out control restores them.
    click(&mut visual, "ZOOM-OUT-a");
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::zoomed(cx)), None);

    click(&mut visual, "CARD-b");
    draw(window, cx);
    for expected in ["c", "d", "b"] {
        click(&mut visual, "BUTTON-next tab");
        draw(window, cx);
        assert_eq!(selected(cx).as_deref(), Some(expected));
        assert_eq!(focused(&mut visual).as_deref(), Some("right"));
    }
    click(&mut visual, "BUTTON-previous tab");
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("d"));
    assert_eq!(focused(&mut visual).as_deref(), Some("right"));
    click(&mut visual, "CARD-a");
    draw(window, cx);
    click(&mut visual, "BUTTON-hide");
    draw(window, cx);
    assert!(cx.update(|cx| shell::is_hidden(cx, "a")));
    assert!(visual.debug_bounds("CARD-a").is_none());
    assert_eq!(focused(&mut visual), None);
}

fn selected(cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| shell::selected_tab(cx, "right").map(|node| node.to_string()))
}

fn focused(visual: &mut VisualTestContext) -> Option<String> {
    visual.update(|window, cx| shell::focused_node(window, cx).map(|node| node.to_string()))
}

fn draw(window: WindowHandle<shell::Shell>, cx: &mut TestAppContext) {
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), Default::default());
}
