use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, px};
#[cfg(not(feature = "dev"))]
use riso_shell as shell;
#[cfg(feature = "dev")]
use riso_shell::dev as shell;

// Select the extension case in a separate process, before GPUI starts threads.
#[gpui::test]
fn controls_reset_the_real_user_layer_case(cx: &mut TestAppContext) {
    if std::env::var("LAYOUT_TOUR_USER_LAYER_TEST_CHILD").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "controls_reset_the_real_user_layer_case",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "user-layer")
            .env("LAYOUT_TOUR_USER_LAYER_TEST_CHILD", "1")
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
    window.update(cx, |_, window, _| window.refresh()).unwrap();
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("USER-LAYER-0").is_some());
    assert_eq!(visual.debug_bounds("CARD-a").unwrap().size.width, px(400.));
    assert_eq!(visual.debug_bounds("CARD-c").unwrap().size.width, px(400.));
    click(&mut visual, "TAB-b");
    draw(window, cx);
    for _ in 0..2 {
        click(&mut visual, "COUNT-b");
        draw(window, cx);
    }
    let start = visual.debug_bounds("TAB-a").unwrap().center();
    let target = visual.debug_bounds("TABBAR-right").unwrap().center();
    visual.simulate_mouse_move(start, None, Default::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(start.x + px(5.), start.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
    assert!(visual.debug_bounds("TAB-a").unwrap().origin.x > px(800.));
    let point = |x| gpui::point(px(x), px(300.));
    visual.simulate_mouse_move(point(400.), None, Default::default());
    visual.simulate_mouse_down(point(400.), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(point(500.), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(point(500.), gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    assert_eq!(visual.debug_bounds("CARD-b").unwrap().size.width, px(300.));
    click(&mut visual, "TAB-d");
    draw(window, cx);
    click(&mut visual, "BUTTON-hide d");
    draw(window, cx);
    assert!(visual.debug_bounds("TAB-d").is_none());
    // The move, the resize, and the hide are each one recorded operation.
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 3);
    assert!(visual.debug_bounds("USER-LAYER-3").is_some());
    click(&mut visual, "BUTTON-reset layout");
    draw(window, cx);
    assert!(visual.debug_bounds("USER-LAYER-0").is_some());
    assert!(visual.debug_bounds("TAB-d").is_some());
    assert!(visual.debug_bounds("COUNT-a").is_some());
    assert!(visual.debug_bounds("COUNT-b").is_none());
    assert!(visual.debug_bounds("COUNT-c").is_some());
    assert_eq!(visual.debug_bounds("CARD-a").unwrap().size.width, px(400.));
    assert_eq!(visual.debug_bounds("CARD-c").unwrap().size.width, px(400.));
    click(&mut visual, "TAB-b");
    draw(window, cx);
    assert!(visual.debug_bounds("COUNT-b-2").is_some());
    click(&mut visual, "BUTTON-reset layout");
    draw(window, cx);
    assert!(
        visual.debug_bounds("COUNT-b-2").is_some(),
        "empty reset keeps selection and state"
    );
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
}

fn draw(window: WindowHandle<shell::Shell>, cx: &mut TestAppContext) {
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
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
