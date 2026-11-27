use gpui::TestAppContext;
use yard_test::{TestApp, capture_trace};

#[cfg(target_os = "macos")]
const TOGGLE_KEY: &str = "cmd-shift-p";
#[cfg(not(target_os = "macos"))]
const TOGGLE_KEY: &str = "ctrl-shift-p";

#[gpui::test]
fn shortcut_opens_the_command_palette(cx: &mut TestAppContext) {
    capture_trace(|trace| {
        let app = TestApp::builder()
            .extension("command_palette", command_palette::init)
            .boot(cx);
        let mut visual = app.visual(cx);
        visual.simulate_keystrokes(TOGGLE_KEY);
        app.draw(cx);

        assert!(visual.update(|window, cx| yard_shell::has_modal(window, cx)));
        let group = trace
            .group("modal-show")
            .expect("the palette opens a modal");
        assert!(trace.lines_in(&group).iter().any(|line| {
            line.level == tracing::Level::DEBUG
                && line.target.starts_with("command_palette")
                && line.message == "commands listed"
        }));
        trace.assert_no_errors();
    });
}
