use gpui::TestAppContext;
use yard_test::{TestApp, capture_logs};

// Use this test as a starting point for testing your application.
#[gpui::test]
fn opens_one_window_without_errors(cx: &mut TestAppContext) {
    // Capture boot here to include errors logged by extension initializers.
    capture_logs(|trace| {
        let mut builder = TestApp::builder().application({{crate}}::startup::application());
        for &(name, init) in {{crate}}::startup::EXTENSIONS {
            builder = builder.extension(name, init);
        }
        let app = builder.menus({{crate}}::menus::menus).boot(cx);
        assert_eq!(cx.windows().len(), 1, "startup opens exactly one window");

        app.draw(cx);
        trace.assert_no_errors();
    });
}
