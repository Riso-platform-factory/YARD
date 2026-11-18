use gpui::{App, AppContext, Context, Render, TestAppContext, Window, WindowHandle, WindowOptions};
use riso_shell as shell;
use std::{cell::Cell, fs, path::Path, rc::Rc, time::Duration};

struct Card {
    value: u64,
    captures: Rc<Cell<usize>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}

impl shell::NodeState for Card {
    fn save_state(&self, _: &App) -> serde_json::Value {
        self.captures.set(self.captures.get() + 1);
        self.value.into()
    }

    fn restore_state(&mut self, _: serde_json::Value, _: &mut Window, _: &mut Context<Self>) {}
}

fn register(cx: &mut TestAppContext) -> Rc<Cell<usize>> {
    let captures = Rc::new(Cell::new(0));
    cx.update(|cx| {
        riso_ui::init(cx);
        let extension = shell::Extension::new(cx, "rules").unwrap();
        let captures = captures.clone();
        extension.register(cx, "rules.card", &[], move |_, _, window, cx| {
            let value = window.window_handle().window_id().as_u64();
            cx.new(|_| Card {
                value,
                captures: captures.clone(),
            })
            .into()
        });
        extension.register_state::<Card>(cx, "rules.card");
        extension.place_root(cx, "card", "rules.card");
    });
    captures
}

fn open(cx: &mut TestAppContext) -> WindowHandle<shell::Shell> {
    open_with_options(cx, WindowOptions::default())
}

fn open_with_options(
    cx: &mut TestAppContext,
    options: WindowOptions,
) -> WindowHandle<shell::Shell> {
    let window = cx.update(|cx| cx.open_window(options, shell::new_shell).unwrap());
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    window
}

fn settle(cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
}

#[gpui::test]
fn a_change_without_a_state_file_does_nothing(cx: &mut TestAppContext) {
    let captures = register(cx);
    let window = open(cx);

    window
        .update(cx, |_, _, cx| shell::state_changed(cx))
        .unwrap();
    settle(cx);
    assert_eq!(captures.get(), 0);
}

#[gpui::test]
fn two_windows_changing_at_once_save_one_file(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    let captures = register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let first = open(cx);
    let second = open(cx);
    let ids = [first, second].map(|window| {
        window
            .update(cx, |_, window, cx| {
                shell::state_changed(cx);
                window.window_handle().window_id().as_u64()
            })
            .unwrap()
    });

    settle(cx);
    assert_eq!(captures.get(), 2);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let windows = saved["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 2);
    for (window, id) in windows.iter().zip(ids) {
        assert_eq!(window["card"]["state"].as_u64(), Some(id));
    }
}

#[gpui::test]
fn a_change_after_the_last_window_closes_does_not_save_twice(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    let captures = register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let window = open(cx);
    window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();

    cx.update(shell::state_changed);
    settle(cx);
    assert!(
        !file.exists(),
        "a change after the last close wrote the file"
    );
    cx.quit();
    cx.run_until_parked();
    assert_eq!(captures.get(), 1);
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(saved["windows"].as_array().unwrap().len(), 1);
}

fn window_id(window: WindowHandle<shell::Shell>) -> u64 {
    gpui::AnyWindowHandle::from(window).window_id().as_u64()
}

fn saved_windows(path: &Path) -> Vec<serde_json::Value> {
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    saved["windows"].as_array().unwrap().clone()
}

#[gpui::test]
fn opening_a_window_saves_after_the_change_wait(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let window = open(cx);

    cx.executor().advance_clock(Duration::from_millis(299));
    assert!(!file.exists());
    cx.executor().advance_clock(Duration::from_millis(1));
    let windows = saved_windows(&file);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["card"]["state"], window_id(window));
}

#[gpui::test]
fn reopening_supersedes_the_last_closed_window_snapshot(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));

    let closed = open(cx);
    closed
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let reopened = open(cx);

    cx.executor().advance_clock(Duration::from_millis(299));
    assert!(!file.exists());
    cx.executor().advance_clock(Duration::from_millis(1));
    let saved = saved_windows(&file);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["card"]["state"], window_id(reopened));
}

#[gpui::test]
fn a_steady_stream_writes_at_each_two_second_boundary(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let window = open(cx);
    let clock = cx
        .executor()
        .dispatcher()
        .as_test()
        .unwrap()
        .scheduler()
        .clock();
    let mut writes = Vec::new();

    cx.update(shell::state_changed);
    for tick in 1..=120 {
        cx.executor().advance_clock(Duration::from_millis(49));
        assert!(!file.exists(), "wrote by {} ms", tick * 50 - 1);
        clock.advance(Duration::from_millis(1));
        while cx.executor().tick() {
            if file.exists() {
                let windows = saved_windows(&file);
                assert_eq!(windows[0]["card"]["state"], window_id(window));
                writes.push(tick * 50);
                // Clear each completed write before the next scheduler task runs.
                fs::remove_file(&file).unwrap();
            }
        }
        if tick % 2 == 1 {
            cx.update(shell::state_changed);
        }
    }

    assert_eq!(writes, [2000, 4000, 6000]);
    settle(cx);
    assert!(!file.exists());
}

#[gpui::test]
fn closing_one_of_two_windows_keeps_state_in_step_with_the_session(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state.json");
    let session = temp.path().join("session.json");
    register(cx);
    cx.update(|cx| {
        shell::track_session(cx, session.clone());
        shell::set_state_file(cx, state.clone());
    });
    let open_at = |cx: &mut TestAppContext, x: f32| {
        open_with_options(
            cx,
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    gpui::point(gpui::px(x), gpui::px(90.)),
                    gpui::size(gpui::px(640.), gpui::px(480.)),
                ))),
                ..Default::default()
            },
        )
    };
    let first = open_at(cx, 100.);
    let second = open_at(cx, 300.);
    cx.update(shell::state_changed);
    settle(cx);
    let state_before = saved_windows(&state);
    let session_before = saved_windows(&session);
    assert_eq!(session_before.len(), 2);
    assert_eq!(state_before.len(), 2);
    assert_eq!(session_before[1]["bounds"]["x"], 300.);
    assert_eq!(state_before[1]["card"]["state"], window_id(second));

    first
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();

    cx.executor().advance_clock(Duration::from_millis(299));
    assert_eq!(saved_windows(&state), state_before);
    cx.executor().advance_clock(Duration::from_millis(1));
    assert_eq!(saved_windows(&session), session_before[1..]);
    assert_eq!(saved_windows(&state), state_before[1..]);
}

#[gpui::test]
fn the_next_change_saves_after_a_failed_write(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    let captures = register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let window = open(cx);

    // Atomic rename fails when the destination is a directory.
    fs::create_dir(&file).unwrap();
    cx.update(shell::state_changed);
    settle(cx);
    assert!(file.is_dir());
    assert_eq!(captures.get(), 1);

    fs::remove_dir(&file).unwrap();
    cx.update(shell::state_changed);
    settle(cx);
    assert_eq!(captures.get(), 2);
    assert_eq!(saved_windows(&file)[0]["card"]["state"], window_id(window));
}

#[gpui::test]
fn quitting_with_a_change_write_in_flight_saves_the_quit_snapshot(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("state.json");
    let captures = register(cx);
    cx.update(|cx| shell::set_state_file(cx, file.clone()));
    let first = open(cx);
    cx.update(shell::state_changed);
    cx.run_until_parked();
    assert!(
        cx.executor()
            .dispatcher()
            .as_test()
            .unwrap()
            .advance_clock_to_next_timer()
    );
    while captures.get() == 0 {
        assert!(cx.executor().tick(), "the change save never captured state");
    }
    assert!(!file.exists(), "the change write already finished");

    let second = open(cx);
    assert!(!file.exists(), "the change write finished before quit");
    cx.quit();
    cx.run_until_parked();

    let windows = saved_windows(&file);
    assert_eq!(windows.len(), 2);
    for (saved, window) in windows.iter().zip([first, second]) {
        assert_eq!(saved["card"]["state"], window_id(window));
    }
}
