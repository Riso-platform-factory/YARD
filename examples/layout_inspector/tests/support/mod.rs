use gpui::{AppContext, TestAppContext, WindowHandle, px};
use yard_shell as shell;

pub fn child(test: &str) -> bool {
    if std::env::var("LAYOUT_INSPECTOR_TEST_CHILD").as_deref() == Ok("1") {
        return true;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("LAYOUT_INSPECTOR", "1")
        .env("LAYOUT_INSPECTOR_TEST_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

pub fn setup(app: &mut TestAppContext) -> (shell::Extension, WindowHandle<shell::Shell>) {
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        });
        host.place_root_with_props(cx, "tour_frame", "yard.row", serde_props());
        host.place(cx, "root", "yard.column", "tour_frame", "children");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "b", "tour.card", "root", "children");
        let inspector = shell::Extension::new(cx, "layout_inspector").unwrap();
        layout_inspector::init(cx, &inspector);
        host
    });
    let window = app.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(1200.), px(700.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    (host, window)
}

// Serialize without adding a test-only dependency to the example crate.
fn serde_props()
-> std::collections::BTreeMap<&'static str, std::collections::BTreeMap<&'static str, u32>> {
    [("sizes", [("inspector", 360)].into())].into()
}

pub fn draw(window: WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}
