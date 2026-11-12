#![cfg(feature = "dev")]

use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, px};

// A separate process selects the real extension case before GPUI starts threads.
// It also keeps card renders separate from the unit binary's tracing capture.
#[gpui::test]
fn card_clicks_zoom_and_restore_the_layout(cx: &mut TestAppContext) {
    if std::env::var("LAYOUT_TOUR_ZOOM_TEST_CHILD").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "card_clicks_zoom_and_restore_the_layout",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "zoom")
            .env("LAYOUT_TOUR_ZOOM_TEST_CHILD", "1")
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
        let ext = riso_shell::dev::Extension::new(cx, "layout_tour").unwrap();
        layout_tour::init(cx, &ext);
    });
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(1000.), px(600.)));
    window.update(cx, |_, window, _| window.refresh()).unwrap();
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for _ in 0..2 {
        click(&mut visual, "COUNT-a");
        draw(window, cx);
    }
    let point = |x| gpui::point(px(x), px(300.));
    visual.simulate_mouse_move(point(500.), None, Default::default());
    visual.simulate_mouse_down(point(500.), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(point(600.), gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(point(600.), gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    click(&mut visual, "TAB-c");
    draw(window, cx);
    let a = visual.debug_bounds("COUNT-a").unwrap();
    let c = visual.debug_bounds("COUNT-c").unwrap();
    let tab = visual.debug_bounds("TAB-c").unwrap();
    click(&mut visual, "ZOOM-c");
    draw(window, cx);
    assert_eq!(
        cx.update(|cx| riso_shell::dev::zoomed(cx)).as_deref(),
        Some("c")
    );
    assert!(visual.debug_bounds("COUNT-a").is_none());
    assert!(visual.debug_bounds("TAB-c").is_none());
    assert!(visual.debug_bounds("COUNT-b").is_none());
    assert!(visual.debug_bounds("ZOOM-c").is_none());
    assert!(visual.debug_bounds("ZOOM-OUT-c").is_some());
    assert_eq!(visual.debug_bounds("CARD-c").unwrap().size.width, px(1000.));
    let zoomed_c = visual.debug_bounds("COUNT-c").unwrap();
    assert!(zoomed_c.origin.x < c.origin.x);
    click(&mut visual, "ZOOM-OUT-c");
    draw(window, cx);
    assert!(visual.debug_bounds("ZOOM-c").is_some());
    assert!(visual.debug_bounds("ZOOM-OUT-c").is_none());
    assert!(visual.debug_bounds("COUNT-a-2").is_some());
    assert_eq!(cx.update(|cx| riso_shell::dev::zoomed(cx)), None);
    assert_eq!(visual.debug_bounds("COUNT-a"), Some(a));
    assert_eq!(visual.debug_bounds("COUNT-c"), Some(c));
    assert_eq!(visual.debug_bounds("TAB-c"), Some(tab));
    assert!(visual.debug_bounds("COUNT-b").is_none());
}

fn draw(window: WindowHandle<riso_shell::dev::Shell>, cx: &mut TestAppContext) {
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
