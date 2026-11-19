use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, px};
use yard_shell as shell;

#[gpui::test]
fn context_controls_share_count_refuse_invalid_move_and_reset(cx: &mut TestAppContext) {
    if std::env::var("LAYOUT_TOUR_CONTEXTS_TEST_CHILD").as_deref() != Ok("1") {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "context_controls_share_count_refuse_invalid_move_and_reset",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "contexts")
            .env("LAYOUT_TOUR_CONTEXTS_TEST_CHILD", "1")
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
    cx.simulate_window_resize(window.into(), gpui::size(px(1000.), px(800.)));
    draw(window, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for selector in ["CONTEXT-left-0", "CONTEXT-right-0"] {
        assert!(visual.debug_bounds(selector).is_some());
    }
    for _ in 0..2 {
        click(&mut visual, "CONTEXT-PLUS");
        draw(window, cx);
    }
    for selector in ["CONTEXT-left-2", "CONTEXT-right-2"] {
        assert!(visual.debug_bounds(selector).is_some());
    }
    click(&mut visual, "BUTTON-move left to root");
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
    let outside = visual.debug_bounds("CARD-outside").unwrap();
    click(&mut visual, "BUTTON-move outside into counter");
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
    assert_ne!(visual.debug_bounds("CARD-outside").unwrap(), outside);
    click(&mut visual, "BUTTON-reset layout");
    draw(window, cx);
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
    assert_eq!(visual.debug_bounds("CARD-outside").unwrap(), outside);
    for selector in ["CONTEXT-left-2", "CONTEXT-right-2"] {
        assert!(visual.debug_bounds(selector).is_some());
    }
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
