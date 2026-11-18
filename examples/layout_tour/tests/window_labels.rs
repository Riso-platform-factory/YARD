use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle, WindowOptions, px};
#[cfg(not(feature = "dev"))]
use riso_shell as shell;
#[cfg(feature = "dev")]
use riso_shell::dev as shell;

#[gpui::test]
fn each_window_draws_its_own_user_layer_count(cx: &mut TestAppContext) {
    if std::env::var_os("LAYOUT_TOUR_WINDOW_LABELS_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "each_window_draws_its_own_user_layer_count",
                "--nocapture",
            ])
            .env("LAYOUT_TOUR", "user-layer")
            .env("LAYOUT_TOUR_WINDOW_LABELS_CHILD", "1")
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

    let extension = cx.update(|cx| {
        riso_ui::init(cx);
        let ext = shell::Extension::new(cx, "layout_tour").unwrap();
        layout_tour::init(cx, &ext);
        ext
    });
    let (first, second) = cx.update(|cx| {
        let first = cx
            .open_window(WindowOptions::default(), shell::new_shell)
            .unwrap();
        let second = cx
            .open_window(WindowOptions::default(), shell::new_shell)
            .unwrap();
        (first, second)
    });
    first
        .update(cx, |_, window, cx| {
            extension.add(window, cx, "extra", "layout_tour.card", "left", "children");
        })
        .unwrap();
    second
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    for (window, selector) in [(first, "USER-LAYER-1"), (second, "USER-LAYER-0")] {
        draw(window, cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds(selector).is_some());
    }
}

fn draw(window: WindowHandle<shell::Shell>, cx: &mut TestAppContext) {
    cx.simulate_window_resize(window.into(), gpui::size(px(1200.), px(600.)));
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}
