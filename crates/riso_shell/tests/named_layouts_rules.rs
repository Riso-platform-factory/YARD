use gpui::{AppContext, InteractiveElement, IntoElement, SharedString, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, Once},
};

static TRACES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static INIT: Once = Once::new();
struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        if record.target().starts_with("riso_shell")
            && matches!(record.level(), log::Level::Warn | log::Level::Error)
        {
            match record.level() {
                log::Level::Error => {
                    tracing::event!(target: "named_rules_diagnostics", tracing::Level::ERROR, "{}", record.args())
                }
                log::Level::Warn => {
                    tracing::event!(target: "named_rules_diagnostics", tracing::Level::WARN, "{}", record.args())
                }
                _ => unreachable!(),
            }
            LOGS.lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }
    fn flush(&self) {}
}
fn lock() -> std::sync::MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap();
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    LOGS.lock().unwrap().clear();
    TRACES.lock().unwrap().clear();
    guard
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
    span_stacks: Vec<Vec<String>>,
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
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let label = format!("{}{}", attributes.metadata().name(), fields.0);
        if attributes.metadata().target().starts_with("riso_shell") {
            let mut stack: Vec<String> = captured
                .entered
                .iter()
                .map(|entered| captured.spans[entered].clone())
                .collect();
            stack.push(label.clone());
            captured.span_stacks.push(stack);
        }
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("riso_shell")
            || target.starts_with("riso_layout")
            || target == "named_rules_diagnostics")
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].as_str())
            .unwrap_or("none");
        let line = format!("{group}: {}{}", event.metadata().level(), fields.0);
        TRACES.lock().unwrap().push(line.clone());
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext)) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx));
}

struct Names;
impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        vec!["ada".into(), "bob".into()]
    }
}

struct Card(String);
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .size_full()
            .debug_selector(|| format!("CARD-{}", self.0))
    }
}

type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

fn setup(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let created = views.clone();
    let host = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let view = cx.new(|_| Card(node.to_string()));
            created
                .borrow_mut()
                .insert(node.to_string(), view.downgrade());
            view.into()
        });
        host.place_root(cx, "root", "riso.row");
        for node in ["a", "b", "c"] {
            host.place(cx, node, "tour.card", "root", "children");
        }
        host
    });
    (host, views)
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

fn show(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    host: &shell::Extension,
    name: &str,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| host.show_layout(window, cx, name))
        .unwrap();
    draw(window, app);
}

#[gpui::test]
fn named_add_checks_the_registering_extensions_namespace_at_switch(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            let guest = shell::Extension::new(cx, "guest").unwrap();
            guest.register(cx, "guest.card", &[], |node, _, _, cx| {
                cx.new(|_| Card(node.to_string())).into()
            });
            host.named_layout(cx, "tour.foreign", |layout| {
                layout.add("foreign", "guest.card", "root", "children");
                layout.set_hidden("c", true);
            });
        });
        let window = open(&mut app);
        show(window, &mut app, &host, "tour.foreign");
        assert!(!views.borrow().contains_key("foreign"));
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("CARD-foreign").is_none());
        assert!(visual.debug_bounds("CARD-c").is_none());
        assert!(
            TRACES
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("INFO")
                    && line.contains("skipped")
                    && line.contains("namespace"))
        );
    });
}

struct TempFile(std::path::PathBuf);
impl TempFile {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "riso-named-rules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[gpui::test]
fn a_broken_named_file_warns_and_keeps_the_named_operations(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let temp = TempFile::new();
        let path = temp.0.join("broken.json");
        std::fs::write(&path, "not json").unwrap();
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            shell::set_named_layout_files(cx, move |_| path.clone());
            host.named_layout(cx, "tour.swap", |layout| {
                layout.move_node("c", "root", "children", 0)
            });
        });
        let window = open(&mut app);
        show(window, &mut app, &host, "tour.swap");
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(
            visual.debug_bounds("CARD-c").unwrap().origin.x
                < visual.debug_bounds("CARD-a").unwrap().origin.x
        );
        assert!(
            LOGS.lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("saved layout cannot be read"))
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

#[gpui::test]
fn without_files_each_named_layer_survives_in_memory_and_warns_once(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            host.named_layout(cx, "tour.one", |_| {});
            host.named_layout(cx, "tour.two", |_| {});
        });
        let window = open(&mut app);
        show(window, &mut app, &host, "tour.one");
        app.update(|cx| host.set_hidden(cx, "a", true));
        show(window, &mut app, &host, "tour.two");
        app.update(|cx| host.set_hidden(cx, "b", true));
        show(window, &mut app, &host, "tour.one");
        assert!(app.update(|cx| shell::is_hidden(cx, "a")));
        assert!(!app.update(|cx| shell::is_hidden(cx, "b")));
        show(window, &mut app, &host, "tour.two");
        assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
        assert!(app.update(|cx| shell::is_hidden(cx, "b")));
        assert_eq!(
            LOGS.lock()
                .unwrap()
                .iter()
                .filter(|line| line.contains("memory"))
                .count(),
            1
        );
    });
}

#[gpui::test]
fn focus_is_kept_when_moved_and_returns_to_window_when_hidden(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            host.named_layout(cx, "tour.move", |layout| {
                layout.move_node("b", "root", "children", 0)
            });
            host.named_layout(cx, "tour.hide", |layout| layout.set_hidden("b", true));
        });
        let window = open(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let bounds = visual.debug_bounds("CARD-b").unwrap();
        visual.simulate_click(bounds.center(), gpui::Modifiers::default());
        draw(window, &mut app);
        show(window, &mut app, &host, "tour.move");
        assert_eq!(
            visual
                .update(|window, cx| shell::focused_node(window, cx))
                .unwrap()
                .as_str(),
            "b"
        );
        show(window, &mut app, &host, "tour.hide");
        assert!(
            visual
                .update(|window, cx| shell::focused_node(window, cx))
                .is_none()
        );
        assert!(visual.update(|window, cx| window.focused(cx)).is_some());
    });
}

#[gpui::test]
fn item_copies_keep_identity_while_declared_nodes_stay_and_drop_when_they_leave(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        app.update(|cx| {
            let model = cx.new(|_| Names);
            host.provide_window(cx, model);
            host.place(cx, "items", "riso.row", "root", "children");
            host.item_slot::<Names>(cx, "items", "children");
            host.place(cx, "name", "tour.card", "items", "children");
            host.named_layout(cx, "tour.keep", |layout| {
                layout.move_node("items", "root", "children", 0);
                layout.set_hidden("name@ada", true);
            });
            host.named_layout(cx, "tour.remove", |layout| layout.remove("items"));
        });
        let window = open(&mut app);
        let copy = views.borrow()["name@ada"].upgrade().unwrap().entity_id();
        show(window, &mut app, &host, "tour.keep");
        assert_eq!(
            views.borrow()["name@ada"].upgrade().unwrap().entity_id(),
            copy
        );
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("CARD-name@ada").is_some());
        assert!(
            TRACES
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("skipped") && line.contains("copy of an item slot"))
        );
        show(window, &mut app, &host, "tour.remove");
        assert!(views.borrow()["name@ada"].upgrade().is_none());
        assert!(views.borrow()["name@bob"].upgrade().is_none());
        show(window, &mut app, &host, "riso.default");
        assert_ne!(
            views.borrow()["name@ada"].upgrade().unwrap().entity_id(),
            copy
        );
        assert!(
            LOGS.lock()
                .unwrap()
                .iter()
                .all(|line| !line.contains("ERROR"))
        );
    });
}

fn assert_switch_counts(kept: usize, dropped: usize, created: usize) {
    let lines = TRACES.lock().unwrap();
    let line = lines
        .iter()
        .rev()
        .find(|line| line.contains("message=switched"))
        .unwrap();
    assert!(
        line.split_whitespace()
            .any(|field| field == format!("kept={kept}")),
        "{line}"
    );
    assert!(
        line.split_whitespace()
            .any(|field| field == format!("dropped={dropped}")),
        "{line}"
    );
    assert!(
        line.split_whitespace()
            .any(|field| field == format!("created={created}")),
        "{line}"
    );
}

#[gpui::test]
fn a_same_id_with_another_component_gets_a_new_entity_and_counts_as_replaced(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, views) = setup(&mut app);
        let replacement = views.clone();
        app.update(|cx| {
            host.register(cx, "tour.other", &[], move |node, _, _, cx| {
                let view = cx.new(|_| Card(node.to_string()));
                replacement
                    .borrow_mut()
                    .insert(node.to_string(), view.downgrade());
                view.into()
            });
            host.named_layout(cx, "tour.replace", |layout| {
                layout.remove("a");
                layout.add("a", "tour.other", "root", "children");
            });
            host.named_layout(cx, "tour.exchange", |layout| {
                layout.remove("b");
                layout.add("d", "tour.card", "root", "children");
            });
        });
        let window = open(&mut app);
        let old = views.borrow()["a"].clone();
        let old_id = old.upgrade().unwrap().entity_id();
        let b_id = views.borrow()["b"].upgrade().unwrap().entity_id();
        show(window, &mut app, &host, "tour.replace");
        assert!(old.upgrade().is_none());
        assert_ne!(views.borrow()["a"].upgrade().unwrap().entity_id(), old_id);
        assert_eq!(views.borrow()["b"].upgrade().unwrap().entity_id(), b_id);
        assert_switch_counts(7, 1, 1);
        show(window, &mut app, &host, "riso.default");
        assert_switch_counts(7, 1, 1);
        show(window, &mut app, &host, "tour.exchange");
        assert_switch_counts(7, 1, 1);
    });
}

#[gpui::test]
fn restart_uses_only_the_default_file_even_when_a_named_file_has_operations(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let temp = TempFile::new();
        let default = temp.0.join("layout.json");
        let layouts = temp.0.join("layouts");
        {
            let mut app = cx.new_app();
            let (host, _) = setup(&mut app);
            app.update(|cx| {
                shell::set_layout_file(cx, default.clone());
                let layouts = layouts.clone();
                shell::set_named_layout_files(cx, move |name| layouts.join(format!("{name}.json")));
                host.named_layout(cx, "tour.saved", |_| {});
            });
            let window = open(&mut app);
            app.update(|cx| host.set_hidden(cx, "a", true));
            show(window, &mut app, &host, "tour.saved");
            app.update(|cx| host.set_hidden(cx, "b", true));
        }
        let named = layouts.join("tour.saved.json");
        let saved_named = std::fs::read_to_string(&named).unwrap();
        assert!(saved_named.contains('b'));
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            shell::set_layout_file(cx, default);
            shell::set_named_layout_files(cx, move |name| layouts.join(format!("{name}.json")));
            host.named_layout(cx, "tour.saved", |_| {});
        });
        let window = open(&mut app);
        assert_eq!(
            gpui::AnyWindowHandle::from(window)
                .update(&mut app, |_, window, cx| shell::shown_layout(cx, window))
                .unwrap()
                .as_str(),
            "riso.default"
        );
        assert!(app.update(|cx| shell::is_hidden(cx, "a")));
        assert!(!app.update(|cx| shell::is_hidden(cx, "b")));
        assert_eq!(std::fs::read_to_string(named).unwrap(), saved_named);
        show(window, &mut app, &host, "tour.saved");
        assert!(!app.update(|cx| shell::is_hidden(cx, "a")));
        assert!(app.update(|cx| shell::is_hidden(cx, "b")));
    });
}

#[gpui::test]
fn each_registration_refusal_logs_error_in_its_registration_group(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            host.named_layout(cx, "tour.valid", |_| {});
            for name in ["invalid", "guest.layout", "riso.default", "tour.valid"] {
                host.named_layout(cx, name, |_| panic!("refused registration ran its builder"));
            }
        });
        let lines = TRACES.lock().unwrap();
        for (name, reason) in [
            ("invalid", "invalid named layout"),
            ("guest.layout", "outside extension"),
            ("riso.default", "reserved"),
            ("tour.valid", "already registered"),
        ] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with("named-layout-register ")
                        && line.contains(&format!("name={name:?}"))
                        && line.contains("ERROR")
                        && line.contains(reason)),
                "{lines:#?}"
            );
        }
        assert_eq!(
            LOGS.lock()
                .unwrap()
                .iter()
                .filter(|line| line.starts_with("ERROR"))
                .count(),
            4
        );
    });
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Note {
    text: String,
}
struct NotedCard {
    props: Note,
    told: Rc<RefCell<Vec<String>>>,
}
impl gpui::Render for NotedCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .size_full()
            .debug_selector(|| format!("NOTE-{}", self.props.text))
    }
}
impl shell::PropsView<Note> for NotedCard {
    fn set_props(&mut self, props: Note, _: &mut gpui::Context<Self>) {
        self.told.borrow_mut().push(props.text.clone());
        self.props = props;
    }
}

#[gpui::test]
fn a_kept_node_receives_changed_props_through_its_teller(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        let told = Rc::new(RefCell::new(Vec::new()));
        let views = Rc::new(RefCell::new(Vec::new()));
        let made = views.clone();
        let received = told.clone();
        app.update(|cx| {
            host.register_with_props::<Note, NotedCard>(
                cx,
                "tour.note",
                &[],
                move |props, _, _, _, cx| {
                    made.borrow_mut().push(cx.entity().downgrade());
                    NotedCard {
                        props,
                        told: received.clone(),
                    }
                },
            );
            host.place_with_props(
                cx,
                "note",
                "tour.note",
                "root",
                "children",
                Note {
                    text: "default".into(),
                },
            );
            host.named_layout(cx, "tour.props", |layout| {
                layout.set_props(
                    "note",
                    Note {
                        text: "changed".into(),
                    },
                )
            });
        });
        let window = open(&mut app);
        let entity = views.borrow()[0].upgrade().unwrap().entity_id();
        told.borrow_mut().clear();
        show(window, &mut app, &host, "tour.props");
        assert_eq!(&*told.borrow(), &["changed"]);
        assert_eq!(views.borrow().len(), 1);
        assert_eq!(views.borrow()[0].upgrade().unwrap().entity_id(), entity);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("NOTE-changed").is_some());
        show(window, &mut app, &host, "riso.default");
        assert_eq!(&*told.borrow(), &["changed", "default"]);
    });
}

#[gpui::test]
fn a_named_hide_of_a_required_node_is_skipped_at_info_and_later_operations_apply(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let mut app = cx.new_app();
        let (host, _) = setup(&mut app);
        app.update(|cx| {
            host.register(cx, "tour.required", &[], |node, _, _, cx| {
                cx.new(|_| Card(node.to_string())).into()
            });
            host.require(cx, "tour.required");
            host.place(cx, "required", "tour.required", "root", "children");
            host.named_layout(cx, "tour.required-hide", |layout| {
                layout.set_hidden("required", true);
                layout.set_hidden("c", true);
            });
        });
        let window = open(&mut app);
        show(window, &mut app, &host, "tour.required-hide");
        assert!(!app.update(|cx| shell::is_hidden(cx, "required")));
        assert!(app.update(|cx| shell::is_hidden(cx, "c")));
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert!(visual.debug_bounds("CARD-required").is_some());
        assert!(
            TRACES
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("INFO")
                    && line.contains("skipped")
                    && line.contains("required cannot be hidden: it is required"))
        );
        assert!(
            LOGS.lock()
                .unwrap()
                .iter()
                .all(|line| !line.starts_with("ERROR"))
        );
    });
}
