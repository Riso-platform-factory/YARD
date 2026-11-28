use gpui::{TestAppContext, VisualTestContext, WindowHandle, WindowOptions};

fn open_shell(cx: &mut TestAppContext) -> WindowHandle<yard_shell::Shell> {
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), yard_shell::new_shell)
            .expect("the shell window opens")
    });
    cx.run_until_parked();
    window
}

fn system_title(window: WindowHandle<yard_shell::Shell>, cx: &TestAppContext) -> Option<String> {
    VisualTestContext::from_window(window.into(), cx).window_title()
}

#[gpui::test]
fn set_window_title_sets_the_system_title_of_every_open_window(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let first = open_shell(cx);
    let second = open_shell(cx);

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Notes"));
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Notes"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Notes"));

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Tasks"));
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Tasks"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Tasks"));
}

#[gpui::test]
fn a_window_opened_after_set_window_title_starts_with_that_title(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Notes"));

    let first = open_shell(cx);
    let second = open_shell(cx);
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Notes"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Notes"));

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Tasks"));
    cx.run_until_parked();
    let third = open_shell(cx);
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Tasks"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Tasks"));
    assert_eq!(system_title(third, cx).as_deref(), Some("YARD Tasks"));
}

#[gpui::test]
fn setting_the_same_title_twice_does_not_call_the_system_title_again(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let first = open_shell(cx);
    let second = open_shell(cx);

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Notes"));
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Notes"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Notes"));

    // Overwrite the test platform window's title directly without going through
    // `yard_shell`. Repeating the same title must leave these untouched.
    for window in [first, second] {
        window
            .update(cx, |_, window, _| window.set_window_title("Sentinel"))
            .expect("the shell window is open");
        assert_eq!(system_title(window, cx).as_deref(), Some("Sentinel"));
    }

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Notes"));
    cx.run_until_parked();
    assert_eq!(
        system_title(first, cx).as_deref(),
        Some("Sentinel"),
        "repeating the same title does not call Window::set_window_title again"
    );
    assert_eq!(
        system_title(second, cx).as_deref(),
        Some("Sentinel"),
        "repeating the same title does not call Window::set_window_title again"
    );

    cx.update(|cx| yard_shell::set_window_title(cx, "YARD Tasks"));
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("YARD Tasks"));
    assert_eq!(system_title(second, cx).as_deref(), Some("YARD Tasks"));
}

#[gpui::test]
fn several_titles_in_one_update_leave_the_last_one(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let first = open_shell(cx);
    let second = open_shell(cx);

    cx.update(|cx| {
        yard_shell::set_window_title(cx, "First");
        yard_shell::set_window_title(cx, "Second");
        yard_shell::set_window_title(cx, "Final");
    });
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("Final"));
    assert_eq!(system_title(second, cx).as_deref(), Some("Final"));

    cx.update(|cx| yard_shell::set_window_title(cx, "After"));
    cx.run_until_parked();
    assert_eq!(system_title(first, cx).as_deref(), Some("After"));
    assert_eq!(system_title(second, cx).as_deref(), Some("After"));
}
