#![cfg(feature = "dev")]

use gpui::{App, AppContext as _, Context, IntoElement, Render, TestAppContext, Window};
use riso_shell::dev as shell;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc, sync::Mutex};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct CaptureDiagnostics;

impl log::Log for CaptureDiagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[gpui::test]
fn node_state_rules(cx: &mut TestAppContext) {
    log::set_logger(&CaptureDiagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    null_is_a_saved_state(&mut cx.new_app());
    extra_state_windows_are_ignored(&mut cx.new_app());
    hidden_views_still_save_state(&mut cx.new_app());
    an_absent_node_restores_when_added_later(&mut cx.new_app());
    named_layout_recreates_a_view_without_restoring_twice(&mut cx.new_app());
    omitting_the_middle_window_keeps_session_and_state_aligned(&mut cx.new_app(), false);
    omitting_the_middle_window_keeps_session_and_state_aligned(&mut cx.new_app(), true);
    a_node_hidden_at_start_restores(&mut cx.new_app());
    a_node_without_a_view_keeps_its_state_for_the_next_run(cx);
    missing_registration_preserves_waiting_state(&mut cx.new_app());
    a_wrong_view_type_logs_once_and_drops_saved_state(&mut cx.new_app());
    another_version_is_ignored_at_start(&mut cx.new_app());
}

struct Card(Value);

impl Render for Card {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

impl shell::NodeState for Card {
    fn save_state(&self, _: &App) -> Value {
        self.0.clone()
    }

    fn restore_state(&mut self, state: Value, _: &mut Window, _: &mut Context<Self>) {
        self.0 = state;
    }
}

type Cards = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

fn setup(cx: &mut TestAppContext, file: &Path) -> (shell::Extension, Cards) {
    let cards = Cards::default();
    let ext = cx.update(|cx| {
        riso_ui::init(cx);
        shell::set_state_file(cx, file.to_path_buf());
        let ext = shell::Extension::new(cx, "rules").unwrap();
        let created = cards.clone();
        ext.register(cx, "rules.card", &[], move |node, _, _, cx| {
            let entity = cx.new(|_| Card(Value::Null));
            created
                .borrow_mut()
                .insert(node.to_string(), entity.downgrade());
            entity.into()
        });
        ext.register_state::<Card>(cx, "rules.card");
        ext.place_root(cx, "root", "riso.row");
        ext.place(cx, "card", "rules.card", "root", "children");
        ext
    });
    (ext, cards)
}

fn open(cx: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    })
}

fn read(file: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

fn write(file: &Path, data: Value) {
    std::fs::write(file, data.to_string()).unwrap();
}

fn null_is_a_saved_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("state.json");
    setup(cx, &temp);
    open(cx);
    cx.quit();
    assert_eq!(
        read(&temp)["windows"][0]["card"],
        json!({"component":"rules.card", "state":null})
    );
}

fn extra_state_windows_are_ignored(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("state.json");
    let session_file = temp.with_extension("session.json");
    write(
        &session_file,
        json!({
            "version": 1,
            "windows": [{
                "bounds": {"state":"windowed","x":0,"y":0,"width":800,"height":600},
                "display":null,"layout":"riso.default","layers":{}
            }]
        }),
    );
    write(
        &temp,
        json!({
            "version":1,
            "windows":[
                {"card":{"component":"rules.card","state":1}},
                {"card":{"component":"rules.card","state":2}}
            ]
        }),
    );
    setup(cx, &temp);
    cx.update(|cx| {
        let session = shell::saved_session(cx, &session_file).unwrap();
        cx.open_window(gpui::WindowOptions::default(), |window, cx| {
            shell::new_session_shell(window, cx, &session[0])
        })
        .unwrap();
    });
    open(cx);
    cx.quit();
    let saved = read(&temp);
    assert_eq!(saved["windows"][0]["card"]["state"], 1);
    assert_eq!(saved["windows"][1]["card"]["state"], Value::Null);
}

fn hidden_views_still_save_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("state.json");
    let (ext, cards) = setup(cx, &temp);
    open(cx);
    let card = cards.borrow()["card"].upgrade().unwrap();
    cx.update(|cx| {
        card.update(cx, |card, _| card.0 = json!(3));
        ext.set_hidden(cx, "card", true);
        assert!(shell::is_hidden(cx, "card"));
    });
    cx.quit();
    assert_eq!(read(&temp)["windows"][0]["card"]["state"], 3);
}

fn an_absent_node_restores_when_added_later(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let temp = dir.path().join("state.json");
    write(
        &temp,
        json!({
            "version":1,
            "windows":[{"absent":{"component":"rules.card","state":9}}]
        }),
    );
    let (ext, cards) = setup(cx, &temp);
    let window = open(cx);
    window
        .update(cx, |_, window, cx| {
            ext.add(window, cx, "absent", "rules.card", "root", "children");
        })
        .unwrap();
    assert_eq!(card_state(cx, &cards, "absent"), 9);
    cx.quit();
    assert_eq!(read(&temp)["windows"][0]["absent"]["state"], 9);
}

fn card_state(cx: &mut TestAppContext, cards: &Cards, node: &str) -> Value {
    let card = cards.borrow()[node].upgrade().unwrap();
    cx.update(|cx| card.read(cx).0.clone())
}

fn write_saved_node(file: &Path, node: &str, component: &str, version: u32) {
    write(
        file,
        json!({
            "version": version,
            "windows": [{node: {"component": component, "state": 9}}],
        }),
    );
}

fn named_layout_recreates_a_view_without_restoring_twice(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "named", "rules.card", 1);
    let (ext, cards) = setup(cx, &file);
    cx.update(|cx| {
        ext.named_layout(cx, "rules.with-card", |layout| {
            layout.add("named", "rules.card", "root", "children");
        });
    });
    let window = open(cx);
    window
        .update(cx, |_, window, cx| {
            ext.show_layout(window, cx, "rules.with-card")
        })
        .unwrap();
    assert_eq!(card_state(cx, &cards, "named"), 9);
    let first = cards.borrow()["named"].clone();
    window
        .update(cx, |_, window, cx| {
            ext.show_layout(window, cx, "riso.default")
        })
        .unwrap();
    assert!(
        first.upgrade().is_none(),
        "switching away should drop the view"
    );
    window
        .update(cx, |_, window, cx| {
            ext.show_layout(window, cx, "rules.with-card")
        })
        .unwrap();
    assert_eq!(card_state(cx, &cards, "named"), Value::Null);
    cx.quit();
}

fn omitting_the_middle_window_keeps_session_and_state_aligned(
    cx: &mut TestAppContext,
    retain_middle_shell: bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");
    let session_file = dir.path().join("session.json");
    let (ext, cards) = setup(cx, &state_file);
    let names = ["rules.first", "rules.middle", "rules.third"];
    cx.update(|cx| {
        for name in names {
            ext.named_layout(cx, name, |_| ());
        }
        shell::track_session(cx, session_file.clone());
    });
    let mut windows = Vec::new();
    let middle_shell = Rc::new(RefCell::new(None));
    for (index, name) in names.into_iter().enumerate() {
        let keep = middle_shell.clone();
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), move |window, cx| {
                let shell = shell::new_shell(window, cx);
                if retain_middle_shell && index == 1 {
                    // Keep the shell's release callback from removing this window from the save lists.
                    *keep.borrow_mut() = Some(shell.clone());
                }
                shell
            })
            .unwrap()
        });
        let card = cards.borrow()["card"].upgrade().unwrap();
        window
            .update(cx, |_, window, cx| {
                ext.show_layout(window, cx, name);
                card.update(cx, |card, _| card.0 = json!(index + 1));
            })
            .unwrap();
        windows.push(window);
    }
    let diagnostics_start = DIAGNOSTICS.lock().unwrap().len();
    windows[1]
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(windows[1].update(cx, |_, _, _| ()).is_err());
    cx.quit();
    assert_eq!(middle_shell.borrow().is_some(), retain_middle_shell);
    if retain_middle_shell {
        let diagnostics = DIAGNOSTICS.lock().unwrap();
        let warning = format!(
            "window {} is left out of the session",
            windows[1].window_id().as_u64()
        );
        assert!(
            diagnostics[diagnostics_start..]
                .iter()
                .any(|(level, message)| *level == log::Level::Warn && message.starts_with(&warning))
        );
    }
    let session = read(&session_file);
    let states = read(&state_file);
    assert_eq!(session["windows"].as_array().unwrap().len(), 2);
    assert_eq!(states["windows"].as_array().unwrap().len(), 2);
    for (index, (layout, state)) in [("rules.first", 1), ("rules.third", 3)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(session["windows"][index]["layout"], layout);
        assert_eq!(states["windows"][index]["card"]["state"], state);
    }
}

fn a_node_hidden_at_start_restores(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "card", "rules.card", 1);
    let (ext, cards) = setup(cx, &file);
    cx.update(|cx| ext.set_hidden(cx, "card", true));
    let window = open(cx);
    window
        .update(cx, |_, _, cx| {
            assert!(shell::is_hidden(cx, "card"));
            ext.set_hidden(cx, "card", false);
        })
        .unwrap();
    assert_eq!(card_state(cx, &cards, "card"), 9);
    cx.quit();
}

fn a_node_without_a_view_keeps_its_state_for_the_next_run(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "absent", "rules.card", 1);

    let mut first = cx.new_app();
    let (_, cards) = setup(&mut first, &file);
    open(&mut first);
    assert!(!cards.borrow().contains_key("absent"));
    first.quit();
    assert_eq!(
        read(&file)["windows"][0]["absent"],
        json!({"component":"rules.card", "state":9})
    );

    let mut next = cx.new_app();
    let (ext, cards) = setup(&mut next, &file);
    next.update(|cx| ext.place(cx, "absent", "rules.card", "root", "children"));
    open(&mut next);
    assert_eq!(card_state(&mut next, &cards, "absent"), 9);
    next.quit();
}

fn missing_registration_preserves_waiting_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "unregistered", "rules.unregistered", 1);
    let (ext, _) = setup(cx, &file);
    cx.update(|cx| {
        ext.register(cx, "rules.unregistered", &[], |_, _, _, cx| {
            cx.new(|_| Card(Value::Null)).into()
        });
        ext.place(cx, "unregistered", "rules.unregistered", "root", "children");
    });
    open(cx);
    cx.quit();
    assert_eq!(
        read(&file)["windows"][0]["unregistered"],
        json!({"component":"rules.unregistered", "state":9})
    );
}

struct OtherView;

impl Render for OtherView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

fn a_wrong_view_type_logs_once_and_drops_saved_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "wrong", "rules.wrong", 1);
    let (ext, _) = setup(cx, &file);
    cx.update(|cx| {
        ext.register(cx, "rules.wrong", &[], |_, _, _, cx| {
            cx.new(|_| OtherView).into()
        });
        ext.register_state::<Card>(cx, "rules.wrong");
        ext.named_layout(cx, "rules.wrong-view", |layout| {
            layout.add("wrong", "rules.wrong", "root", "children");
        });
    });
    let window = open(cx);
    window
        .update(cx, |_, window, cx| {
            ext.show_layout(window, cx, "rules.wrong-view");
            ext.show_layout(window, cx, "riso.default");
            ext.show_layout(window, cx, "rules.wrong-view");
        })
        .unwrap();
    cx.quit();
    assert!(read(&file)["windows"][0].get("wrong").is_none());
    assert_eq!(
        DIAGNOSTICS
            .lock()
            .unwrap()
            .iter()
            .filter(|(level, message)| {
                *level == log::Level::Error
                    && message == "view of node wrong is not the type that gives its state"
            })
            .count(),
        1
    );
}

fn another_version_is_ignored_at_start(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("state.json");
    write_saved_node(&file, "card", "rules.card", 42);
    let start = DIAGNOSTICS.lock().unwrap().len();
    let (_, cards) = setup(cx, &file);
    open(cx);
    assert_eq!(card_state(cx, &cards, "card"), Value::Null);
    {
        let diagnostics = DIAGNOSTICS.lock().unwrap();
        let new = &diagnostics[start..];
        assert!(
            new.iter().any(|(level, message)| {
                *level == log::Level::Warn
                    && message.starts_with("the saved state cannot be read")
                    && message.contains("42")
            }),
            "{new:?}"
        );
        assert!(
            !new.iter().any(|(level, _)| *level == log::Level::Error),
            "{new:?}"
        );
    }
    cx.quit();
    assert_eq!(read(&file)["version"], 1);
}
