use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, px};
use yard_shell as shell;

#[gpui::test]
fn select_t20_scrolls_it_into_view_and_records_nothing(cx: &mut TestAppContext) {
    // Select the real extension case before GPUI starts threads in a child process.
    if std::env::var("LAYOUT_TOUR_TAB_OVERFLOW_TEST_CHILD").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "select_t20_scrolls_it_into_view_and_records_nothing",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "tab-overflow")
            .env("LAYOUT_TOUR_TAB_OVERFLOW_TEST_CHILD", "1")
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
        yard_ui::init(cx);
        let ext = shell::Extension::new(cx, "layout_tour").unwrap();
        layout_tour::init(cx, &ext);
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(640.), px(600.)));
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_eq!(selected(cx).as_deref(), Some("t01"));
    assert!(!in_view(&mut visual, "TAB-t20"));

    click(&mut visual, "BUTTON-select t20");
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("t20"));
    assert!(in_view(&mut visual, "TAB-t20"));

    // From a tab in the middle, too.
    click(&mut visual, "TAB-t19");
    draw(window, cx);
    click(&mut visual, "TAB-t16");
    draw(window, cx);
    click(&mut visual, "BUTTON-select t20");
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("t20"));
    assert!(in_view(&mut visual, "TAB-t20"));

    // The next tab after t20 is t01: the bar scrolls back to the start.
    visual.dispatch_action(shell::SelectNextTab);
    draw(window, cx);
    assert_eq!(selected(cx).as_deref(), Some("t01"));
    assert!(in_view(&mut visual, "TAB-t01"));
    assert!(visual.debug_bounds("USER-LAYER-0").is_some());
}

fn in_view(visual: &mut VisualTestContext, selector: &'static str) -> bool {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    bounds.origin.x >= px(-0.5) && bounds.right() <= px(640.5)
}

fn selected(cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
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
