#![cfg(feature = "dev")]

use gpui::{
    AppContext, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    TestAppContext, px,
};
use riso_shell::dev as shell;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

static LOCK: Mutex<()> = Mutex::new(());

type Handle = gpui::WindowHandle<shell::Shell>;

struct Card(String);
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.0))
            .size_full()
    }
}

struct Container(shell::Slots);
impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().flex().children(
            self.0
                .views("children")
                .into_iter()
                .map(|view| gpui::div().flex_1().min_w_0().child(view)),
        )
    }
}

struct Names(Vec<SharedString>);
impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}
impl shell::ItemTabs for Names {
    fn close_item(
        &mut self,
        key: &SharedString,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.0.retain(|item| item != key);
        cx.notify();
    }
    fn new_item(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.0.push("new".into());
        cx.notify();
    }
}

fn setup(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let extension = shell::Extension::new(cx, "tour").unwrap();
        extension.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card(node.to_string())).into()
        });
        extension.register(cx, "tour.container", &["children"], |_, slots, _, cx| {
            cx.new(|_| Container(slots)).into()
        });
        extension.place_root(cx, "root", "tour.container");
        extension
    })
}
fn draw(window: Handle, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}
fn open(app: &mut TestAppContext) -> Handle {
    let window = app.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}
fn activate(window: Handle, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
}
fn click(window: Handle, app: &mut TestAppContext, selector: &'static str) {
    let mut visual = gpui::VisualTestContext::from_window(window.into(), app);
    let at = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
        .center();
    visual.simulate_click(at, Default::default());
    app.run_until_parked();
    draw(window, app);
}
fn ids(app: &mut TestAppContext) -> Vec<String> {
    app.update(|cx| {
        shell::inspect(cx)
            .into_iter()
            .map(|node| node.id.to_string())
            .collect()
    })
}

#[gpui::test]
fn cold_reads_are_empty_and_closing_does_not_reopen_startup_placement(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    app.update(|cx| {
        assert!(shell::inspect(cx).is_empty());
        assert_eq!(shell::user_layer_len(cx), 0);
        assert!(!shell::is_hidden(cx, "a"));
        assert!(!shell::is_empty(cx, "a"));
        assert!(shell::zoomed(cx).is_none());
        assert!(shell::selected_tab(cx, "tabs").is_none());
        assert!(shell::node_bounds(cx, "a").is_none());
        assert!(shell::focus_handle(cx, "a").is_none());
        assert!(shell::context::<Names>(cx, "a").is_none());
        assert!(shell::item(cx, "a@ada").is_none());
    });
    let extension = setup(&mut app);
    app.update(|cx| extension.place(cx, "a", "tour.card", "root", "children"));
    let window = open(&mut app);
    window
        .update(&mut app, |_, window, _| window.remove_window())
        .unwrap();
    app.run_until_parked();
    app.update(|cx| {
        extension.set_zoomed(cx, "a");
        extension.zoom_out(cx);
        extension.place(cx, "late", "tour.card", "root", "children");
        assert!(shell::inspect(cx).is_empty());
        assert!(shell::zoomed(cx).is_none());
    });
    let window = open(&mut app);
    activate(window, &mut app);
    assert!(!ids(&mut app).contains(&"late".to_string()));
}

#[gpui::test]
fn dragging_in_the_second_window_keeps_the_first_tree_and_focus(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        for tabs in ["left", "right"] {
            extension.place(cx, tabs, "riso.tabs", "root", "children");
        }
        extension.place(cx, "a", "tour.card", "left", "children");
        extension.place(cx, "b", "tour.card", "right", "children");
    });
    let first = open(&mut app);
    let second = open(&mut app);
    activate(first, &mut app);
    first
        .update(&mut app, |_, window, cx| {
            shell::focus_handle(cx, "a").unwrap().focus(window, cx)
        })
        .unwrap();
    activate(second, &mut app);
    second
        .update(&mut app, |_, window, cx| {
            shell::focus_handle(cx, "b").unwrap().focus(window, cx)
        })
        .unwrap();
    assert_eq!(
        second
            .update(&mut app, |_, window, cx| shell::focused_node(window, cx))
            .unwrap()
            .unwrap()
            .as_str(),
        "b"
    );
    let mut visual = gpui::VisualTestContext::from_window(second.into(), &app);
    let from = visual.debug_bounds("TAB-a").unwrap().center();
    let to = visual.debug_bounds("TABBAR-right").unwrap().center();
    visual.simulate_mouse_move(from, None, Default::default());
    visual.simulate_mouse_down(from, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(from.x + px(5.), from.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(second, &mut app);
    visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
    draw(second, &mut app);
    visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
    app.run_until_parked();
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
    assert_eq!(ids(&mut app), ["root", "left", "right", "b", "a"]);
    activate(first, &mut app);
    assert_eq!(ids(&mut app), ["root", "left", "a", "right", "b"]);
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    assert_eq!(
        first
            .update(&mut app, |_, window, cx| shell::focused_node(window, cx))
            .unwrap()
            .unwrap()
            .as_str(),
        "a"
    );
    activate(second, &mut app);
    assert_eq!(
        second
            .update(&mut app, |_, window, cx| shell::focused_node(window, cx))
            .unwrap()
            .unwrap()
            .as_str(),
        "left"
    );
}

#[gpui::test]
fn toggles_and_item_tabs_use_their_own_window_even_when_another_is_active(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        // The root provides each window's independent model for its item tabs.
        extension.provide::<Names>(cx, "tour.container", |_, cx| {
            cx.new(|_| Names(vec!["ada".into(), "bob".into()]))
        });
        extension.place(cx, "tabs", "riso.tabs", "root", "children");
        extension.item_tabs::<Names>(cx, "tabs");
        extension.place(cx, "name", "tour.card", "tabs", "children");
        extension.place(cx, "card", "tour.card", "root", "children");
        extension.place_with_props(
            cx,
            "toggle",
            "riso.toggle",
            "status_bar",
            "left",
            serde_json::json!({"node":"card", "icon":"info"}),
        );
    });
    let first = open(&mut app);
    let second = open(&mut app);
    activate(first, &mut app);
    let first_model = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
    activate(second, &mut app);
    let second_model = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
    assert_ne!(first_model.entity_id(), second_model.entity_id());
    activate(first, &mut app);
    draw(second, &mut app);
    click(second, &mut app, "NEW-tabs");
    app.update(|cx| {
        assert_eq!(first_model.read(cx).0.len(), 2);
        assert_eq!(second_model.read(cx).0.len(), 3);
    });
    assert!(!ids(&mut app).contains(&"name@new".into()));
    activate(second, &mut app);
    assert!(ids(&mut app).contains(&"name@new".into()));
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs"))
            .unwrap()
            .as_str(),
        "name@new"
    );
    let bounds = app.update(|cx| shell::node_bounds(cx, "toggle").unwrap());
    activate(first, &mut app);
    let mut visual = gpui::VisualTestContext::from_window(second.into(), &app);
    visual.simulate_click(bounds.center(), Default::default());
    app.run_until_parked();
    assert!(!app.update(|cx| shell::is_hidden(cx, "card")));
    activate(second, &mut app);
    assert!(app.update(|cx| shell::is_hidden(cx, "card")));
    activate(first, &mut app);
    assert!(!ids(&mut app).contains(&"name@new".into()));
    assert!(!app.update(|cx| shell::is_hidden(cx, "card")));
    assert_eq!(
        app.update(|cx| shell::selected_tab(cx, "tabs"))
            .unwrap()
            .as_str(),
        "name@ada"
    );
}

#[gpui::test]
fn secondary_named_layers_stay_in_memory_and_pending_model_work_dies_on_close(
    cx: &mut TestAppContext,
) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        extension.provide::<Names>(cx, "tour.container", |_, cx| {
            cx.new(|_| Names(vec!["ada".into()]))
        });
        extension.place(cx, "tabs", "riso.tabs", "root", "children");
        extension.item_tabs::<Names>(cx, "tabs");
        extension.place(cx, "name", "tour.card", "tabs", "children");
        extension.place(cx, "a", "tour.card", "root", "children");
        extension.named_layout(cx, "tour.other", |_| {});
        let path = std::env::temp_dir().join(format!(
            "riso-window-tree-never-read-{}-{:?}.json",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        shell::set_named_layout_files(cx, move |_| path.clone());
    });
    let first = open(&mut app);
    let second = open(&mut app);
    activate(second, &mut app);
    second
        .update(&mut app, |_, window, cx| {
            extension.show_layout(window, cx, "tour.other")
        })
        .unwrap();
    app.update(|cx| extension.set_hidden(cx, "a", true));
    second
        .update(&mut app, |_, window, cx| {
            extension.show_layout(window, cx, "riso.default");
            assert!(!shell::is_hidden(cx, "a"));
            extension.show_layout(window, cx, "tour.other");
            assert!(shell::is_hidden(cx, "a"));
        })
        .unwrap();
    let model = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
    let calls = Rc::new(RefCell::new(0));
    let observed = calls.clone();
    let _subscription =
        app.update(|cx| shell::observe_layout(cx, move |_| *observed.borrow_mut() += 1));
    second
        .update(&mut app, |_, window, cx| {
            extension.set_hidden(cx, "a", false);
            model.update(cx, |model, cx| {
                model.0.push("pending".into());
                cx.notify();
            });
            window.remove_window();
        })
        .unwrap();
    app.run_until_parked();
    let before = *calls.borrow();
    app.update(|cx| model.update(cx, |_, cx| cx.notify()));
    app.run_until_parked();
    assert_eq!(*calls.borrow(), before);
    activate(first, &mut app);
    assert!(!ids(&mut app).contains(&"name@pending".into()));
}

#[gpui::test]
fn background_model_observers_inspect_the_active_window_and_calls_keep_last_active(
    cx: &mut TestAppContext,
) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        extension.provide::<Names>(cx, "tour.container", |_, cx| {
            cx.new(|_| Names(vec!["ada".into()]))
        });
        extension.place(cx, "tabs", "riso.tabs", "root", "children");
        extension.item_tabs::<Names>(cx, "tabs");
        extension.place(cx, "name", "tour.card", "tabs", "children");
        extension.place(cx, "a", "tour.card", "root", "children");
    });
    let first = open(&mut app);
    let second = open(&mut app);
    activate(second, &mut app);
    let model = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
    activate(first, &mut app);
    let snapshots = Rc::new(RefCell::new(Vec::new()));
    let captured = snapshots.clone();
    let _subscription = app.update(|cx| {
        shell::observe_layout(cx, move |cx| {
            captured.borrow_mut().push(shell::inspect(cx));
        })
    });
    app.update(|cx| {
        model.update(cx, |model, cx| {
            model.0.push("background".into());
            cx.notify();
        })
    });
    assert_eq!(snapshots.borrow().len(), 1);
    assert!(
        !snapshots.borrow()[0]
            .iter()
            .any(|node| node.id == "name@background")
    );
    activate(second, &mut app);
    assert!(ids(&mut app).contains(&"name@background".into()));
    let mut visual = gpui::VisualTestContext::from_window(second.into(), &app);
    visual.deactivate_window();
    app.run_until_parked();
    app.update(|cx| {
        assert!(cx.active_window().is_none());
        extension.set_hidden(cx, "a", true);
        assert!(shell::is_hidden(cx, "a"));
    });
    activate(first, &mut app);
    assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
}

#[gpui::test]
fn closing_main_after_a_save_discards_deferred_model_sync(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    struct SavedLayout(std::path::PathBuf);
    impl Drop for SavedLayout {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let directory = std::env::temp_dir().join(format!(
        ".window-tree-rules-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir(&directory).unwrap();
    let saved = SavedLayout(directory);
    let file = saved.0.join("layout.json");
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        shell::set_layout_file(cx, file.clone());
        extension.provide::<Names>(cx, "tour.container", |_, cx| {
            cx.new(|_| Names(vec!["ada".into()]))
        });
        extension.place(cx, "tabs", "riso.tabs", "root", "children");
        extension.item_tabs::<Names>(cx, "tabs");
        extension.place(cx, "name", "tour.card", "tabs", "children");
        extension.place(cx, "a", "tour.card", "root", "children");
    });
    let main = open(&mut app);
    let survivor = open(&mut app);
    activate(main, &mut app);
    let model = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
    main.update(&mut app, |_, window, cx| {
        extension.set_hidden(cx, "a", true);
        assert!(file.exists(), "the shell saves synchronously before close");
        model.update(cx, |model, cx| {
            model.0.push("pending".into());
            cx.notify();
        });
        window.remove_window();
    })
    .unwrap();
    app.run_until_parked();
    activate(survivor, &mut app);
    assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
    assert!(!ids(&mut app).contains(&"name@pending".into()));
    let operations = riso_layout::decode(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(operations.len(), 1);
    assert!(
        matches!(&operations[0], riso_layout::Operation::SetHidden { node, hidden: true } if node.as_str() == "a")
    );
}

#[gpui::test]
fn closing_active_uses_the_last_active_survivor_among_three_windows(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| extension.place(cx, "a", "tour.card", "root", "children"));
    let first = open(&mut app);
    let second = open(&mut app);
    let third = open(&mut app);
    activate(second, &mut app);
    activate(third, &mut app);
    app.update(|cx| extension.set_hidden(cx, "a", true));
    activate(first, &mut app);
    first
        .update(&mut app, |_, window, _| window.remove_window())
        .unwrap();
    app.run_until_parked();
    assert!(
        app.update(|cx| shell::is_hidden(cx, "a")),
        "third was active more recently than second"
    );
    third
        .update(&mut app, |_, window, _| window.remove_window())
        .unwrap();
    app.run_until_parked();
    assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
}

#[gpui::test]
fn late_item_declarations_and_component_registration_reach_both_windows(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| {
        extension.provide::<Names>(cx, "tour.container", |_, cx| {
            cx.new(|_| Names(vec!["ada".into(), "bob".into()]))
        });
        extension.place(cx, "a", "tour.card", "root", "children");
        extension.place(cx, "late", "tour.late", "root", "children");
    });
    let first = open(&mut app);
    let second = open(&mut app);
    activate(first, &mut app);
    app.update(|cx| extension.item_slot::<Names>(cx, "root", "children"));
    for window in [first, second] {
        activate(window, &mut app);
        assert!(ids(&mut app).contains(&"a@ada".into()));
        draw(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("CARD-a@ada").is_some());
        assert!(visual.debug_bounds("CARD-late@ada").is_none());
    }
    activate(first, &mut app);
    app.update(|cx| {
        extension.register(cx, "tour.late", &[], |node, _, _, cx| {
            cx.new(|_| Card(node.to_string())).into()
        })
    });
    for window in [first, second] {
        draw(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("CARD-late@ada").is_some());
        assert!(visual.debug_bounds("CARD-late@bob").is_some());
    }
}

#[gpui::test]
fn window_models_registered_after_open_reach_existing_and_later_windows(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    app.update(|cx| extension.place(cx, "a", "tour.card", "root", "children"));
    let first = open(&mut app);
    let second = open(&mut app);
    activate(first, &mut app);
    let model = app.update(|cx| {
        let model = cx.new(|_| Names(vec!["shared".into()]));
        extension.provide_window(cx, model.clone());
        model
    });
    let later = open(&mut app);
    for window in [first, second, later] {
        activate(window, &mut app);
        assert_eq!(
            app.update(|cx| shell::context::<Names>(cx, "a"))
                .unwrap()
                .entity_id(),
            model.entity_id()
        );
    }
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    stacks: Vec<Vec<String>>,
    events: Vec<(tracing::Level, String)>,
}
#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);
#[derive(Default)]
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
}
impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        let mut stack = captured
            .entered
            .iter()
            .map(|id| captured.spans[id].clone())
            .collect::<Vec<_>>();
        stack.push(label.clone());
        captured.stacks.push(stack);
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .events
            .push((*event.metadata().level(), fields.0));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

#[gpui::test]
fn observer_work_on_another_window_has_one_group_with_that_windows_id(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let extension = setup(&mut app);
        app.update(|cx| extension.place(cx, "a", "tour.card", "root", "children"));
        let first = open(&mut app);
        let second = open(&mut app);
        activate(first, &mut app);
        let once = Rc::new(Cell::new(false));
        let observer_extension = extension.clone();
        let _subscription = app.update(|cx| {
            shell::observe_layout(cx, move |cx| {
                if !once.replace(true) {
                    second
                        .update(cx, |_, window, cx| {
                            observer_extension.reset_layout(window, cx)
                        })
                        .unwrap();
                }
            })
        });
        trace.0.lock().unwrap().stacks.clear();
        app.update(|cx| extension.set_hidden(cx, "a", true));
        let captured = trace.0.lock().unwrap();
        let stack = captured
            .stacks
            .iter()
            .find(|stack| {
                stack
                    .last()
                    .is_some_and(|label| label.starts_with("layout-reset"))
            })
            .expect("observer reset logged");
        assert_eq!(
            stack,
            &[
                format!("window id={}", second.window_id().as_u64()),
                "layout-reset extension=\"tour\"".into()
            ]
        );
    });
}

#[gpui::test]
fn every_read_and_layout_entry_point_logs_no_window_after_the_last_close(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let extension = setup(&mut app);
        app.update(|cx| {
            extension.place(cx, "a", "tour.card", "root", "children");
            extension.named_layout(cx, "tour.other", |_| {});
        });
        let window = open(&mut app);
        window
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        let probe = app.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| gpui::Empty))
                .unwrap()
        });
        let check = |call: &mut dyn FnMut(&mut gpui::App)| {
            trace.0.lock().unwrap().events.clear();
            app.update(|cx| call(cx));
            assert!(
                trace
                    .0
                    .lock()
                    .unwrap()
                    .events
                    .iter()
                    .any(|(level, fields)| *level == tracing::Level::INFO
                        && fields.contains("no window")),
                "missing no window INFO"
            );
        };
        check(&mut |cx| {
            assert!(shell::inspect(cx).is_empty());
        });
        check(&mut |cx| {
            assert_eq!(shell::user_layer_len(cx), 0);
        });
        check(&mut |cx| {
            assert!(!shell::is_hidden(cx, "a"));
        });
        check(&mut |cx| {
            assert!(!shell::is_empty(cx, "a"));
        });
        check(&mut |cx| {
            assert!(shell::zoomed(cx).is_none());
        });
        check(&mut |cx| {
            assert!(shell::selected_tab(cx, "tabs").is_none());
        });
        check(&mut |cx| {
            assert!(shell::node_bounds(cx, "a").is_none());
        });
        check(&mut |cx| {
            assert!(shell::focus_handle(cx, "a").is_none());
        });
        check(&mut |cx| {
            assert!(shell::context::<Names>(cx, "a").is_none());
        });
        check(&mut |cx| {
            assert!(shell::item(cx, "a@ada").is_none());
        });
        check(&mut |cx| extension.set_zoomed(cx, "a"));
        check(&mut |cx| extension.place(cx, "late", "tour.card", "root", "children"));
        check(&mut |cx| {
            probe
                .update(cx, |_, window, cx| {
                    assert_eq!(shell::shown_layout(cx, window).as_str(), "riso.default");
                })
                .unwrap();
        });
        check(&mut |cx| {
            probe
                .update(cx, |_, window, cx| {
                    assert!(shell::focused_node(window, cx).is_none());
                })
                .unwrap();
        });
        check(&mut |cx| {
            probe
                .update(cx, |_, window, cx| {
                    extension.show_layout(window, cx, "tour.other")
                })
                .unwrap();
        });
        check(&mut |cx| {
            probe
                .update(cx, |_, window, cx| extension.reset_layout(window, cx))
                .unwrap();
        });
    });
}

#[gpui::test]
fn close_releases_focus_typed_views_and_node_contexts(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    struct Typed;
    impl gpui::Render for Typed {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            gpui::div().size_full()
        }
    }
    impl shell::PropsView<serde_json::Value> for Typed {
        fn set_props(&mut self, _: serde_json::Value, _: &mut gpui::Context<Self>) {}
    }
    let mut app = cx.new_app();
    let extension = setup(&mut app);
    let views = Rc::new(RefCell::new(Vec::new()));
    let captured = views.clone();
    app.update(|cx| {
        extension.provide::<Names>(cx, "tour.container", |_, cx| cx.new(|_| Names(Vec::new())));
        extension.register_with_props::<serde_json::Value, Typed>(
            cx,
            "tour.typed",
            &[],
            move |_, _, _, _, cx| {
                captured.borrow_mut().push(cx.entity().downgrade());
                Typed
            },
        );
        extension.place(cx, "typed", "tour.typed", "root", "children");
    });
    let window = open(&mut app);
    activate(window, &mut app);
    let (focus, context) = app.update(|cx| {
        (
            shell::focus_handle(cx, "typed").unwrap().downgrade(),
            shell::context::<Names>(cx, "typed").unwrap().downgrade(),
        )
    });
    assert!(focus.upgrade().is_some());
    assert!(context.upgrade().is_some());
    assert!(views.borrow()[0].upgrade().is_some());
    window
        .update(&mut app, |_, window, _| window.remove_window())
        .unwrap();
    app.run_until_parked();
    assert!(
        focus.upgrade().is_none(),
        "closed window retains focus handle"
    );
    assert!(
        context.upgrade().is_none(),
        "closed window retains node context"
    );
    assert!(
        views.borrow()[0].upgrade().is_none(),
        "closed window retains props view/teller"
    );
}
