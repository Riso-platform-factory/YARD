use gpui::{AppContext, IntoElement, ParentElement, Styled, TestAppContext, px};
use serde_json::json;
use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_layout::{NodeId, Operation, Props, decode};
use yard_shell as shell;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-props-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Every test in this file shares one process-wide logger and runs under one
// lock, so a test never sees another test's diagnostic records.
static WARNINGS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            WARNINGS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }
    fn flush(&self) {}
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    let guard = LOCK.lock().unwrap();
    WARNINGS.lock().unwrap().clear();
    guard
}

fn warnings() -> Vec<(log::Level, String)> {
    WARNINGS.lock().unwrap().clone()
}

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

fn object(value: serde_json::Value) -> Props {
    value.as_object().unwrap().clone()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CardProps {
    title: String,
    tint: String,
    count: u32,
}

impl CardProps {
    fn new(title: &str, tint: &str, count: u32) -> Self {
        Self {
            title: title.into(),
            tint: tint.into(),
            count,
        }
    }
}

type Shown = Rc<RefCell<Vec<String>>>;
type Created = Rc<RefCell<Vec<(String, CardProps)>>>;
type Told = Rc<RefCell<Vec<(String, CardProps)>>>;

struct Card {
    node: String,
    slots: shell::Slots,
    shown: Shown,
    told: Told,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::canvas(
                    move |_, _, _| shown.borrow_mut().push(node),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(
                self.slots
                    .views("body")
                    .into_iter()
                    .map(|view| gpui::div().flex_1().min_h_0().child(view)),
            )
    }
}

impl shell::PropsView<CardProps> for Card {
    fn set_props(&mut self, props: CardProps, _: &mut gpui::Context<Self>) {
        self.told.borrow_mut().push((self.node.clone(), props));
    }
}

struct PlainCard {
    node: String,
    shown: Shown,
}

impl gpui::Render for PlainCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div().size_full().child(
            gpui::canvas(
                move |_, _, _| shown.borrow_mut().push(node),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext, shown: &Shown) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app, shown);
    window
}

fn created_props(created: &Created, node: &str) -> Option<CardProps> {
    created
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == node)
        .map(|(_, props)| props.clone())
}

fn told_props(told: &Told, node: &str) -> Vec<CardProps> {
    told.borrow()
        .iter()
        .filter(|(name, _)| name == node)
        .map(|(_, props)| props.clone())
        .collect()
}

fn setup(app: &mut TestAppContext) -> (shell::Extension, shell::Extension, Created, Told, Shown) {
    let created = Created::default();
    let told = Told::default();
    let shown = Shown::default();
    let (host, guest) = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let guest = shell::Extension::new(cx, "guest").unwrap();
        let created = created.clone();
        let told = told.clone();
        let shown = shown.clone();
        let plain_shown = shown.clone();
        host.register_with_props::<CardProps, Card>(
            cx,
            "tour.card",
            &["body"],
            move |props, node, slots, _, _| {
                created.borrow_mut().push((node.to_string(), props));
                Card {
                    node: node.to_string(),
                    slots,
                    shown: shown.clone(),
                    told: told.clone(),
                }
            },
        );
        host.register(cx, "tour.plain", &[], move |node, _, _, cx| {
            let card = cx.new(|_| PlainCard {
                node: node.to_string(),
                shown: plain_shown.clone(),
            });
            card.into()
        });
        host.place_root(cx, "root", "yard.row");
        host.place_with_props(
            cx,
            "alpha",
            "tour.card",
            "root",
            "children",
            json!({"title": "Alpha", "tint": "accent"}),
        );
        host.place(cx, "beta", "tour.card", "root", "children");
        host.place_with_props(
            cx,
            "bad",
            "tour.card",
            "root",
            "children",
            json!({"title": 7, "tint": "accent"}),
        );
        host.place_with_props(
            cx,
            "odd",
            "tour.card",
            "root",
            "children",
            json!({"title": "Odd", "mystery": 1}),
        );
        host.place(cx, "plain", "tour.plain", "root", "children");
        (host, guest)
    });
    (host, guest, created, told, shown)
}

#[gpui::test]
fn views_are_created_from_the_nodes_props(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (_host, _guest, created, _told, shown) = setup(&mut app);
    let _window = open_and_draw(&mut app, &shown);
    assert_eq!(
        created_props(&created, "alpha"),
        Some(CardProps::new("Alpha", "accent", 0))
    );
    // A node placed without props gives its view every default.
    assert_eq!(created_props(&created, "beta"), Some(CardProps::default()));
    // A bad value keeps its default without costing the other keys.
    assert_eq!(
        created_props(&created, "bad"),
        Some(CardProps::new("", "accent", 0))
    );
    // An unknown key is ignored.
    assert_eq!(
        created_props(&created, "odd"),
        Some(CardProps::new("Odd", "", 0))
    );
    // A component registered without props is not given any.
    assert_eq!(created_props(&created, "plain"), None);
    assert_eq!(*shown.borrow(), ["alpha", "beta", "bad", "odd", "plain"]);
}

#[gpui::test]
fn a_bad_value_keeps_its_default_warns_and_costs_no_other_key(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, created, told, shown) = setup(&mut app);
    let _window = open_and_draw(&mut app, &shown);
    assert_eq!(
        created_props(&created, "bad"),
        Some(CardProps::new("", "accent", 0))
    );
    let read_warnings = || -> Vec<String> {
        warnings()
            .into_iter()
            .filter(|(level, message)| {
                *level == log::Level::Warn && message.contains("cannot be read")
            })
            .map(|(_, message)| message)
            .collect()
    };
    let bad_reads = read_warnings();
    assert_eq!(bad_reads.len(), 1, "{bad_reads:?}");
    assert!(
        bad_reads[0].starts_with("prop title of node bad cannot be read:"),
        "{}",
        bad_reads[0]
    );
    assert!(
        bad_reads[0].ends_with("the default is used"),
        "{}",
        bad_reads[0]
    );
    // The props are read again each time they change, with the same rule.
    app.update(|cx| host.set_props(cx, "bad", json!({"title": 9})));
    assert_eq!(read_warnings().len(), 2);
    assert_eq!(told_props(&told, "bad"), [CardProps::new("", "accent", 0)]);
}

#[gpui::test]
fn an_unknown_prop_is_ignored_with_a_warn(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, _guest, created, told, shown) = setup(&mut app);
    let _window = open_and_draw(&mut app, &shown);
    assert_eq!(
        created_props(&created, "odd"),
        Some(CardProps::new("Odd", "", 0))
    );
    let unknown_warnings = || -> Vec<String> {
        warnings()
            .into_iter()
            .filter(|(level, message)| {
                *level == log::Level::Warn && message.contains("unknown prop")
            })
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        unknown_warnings(),
        ["node odd has an unknown prop mystery".to_string()]
    );
    // A set_props is read key by key too: each unknown key warns once.
    app.update(|cx| host.set_props(cx, "odd", json!({"title": "O2", "extra": true})));
    assert_eq!(told_props(&told, "odd"), [CardProps::new("O2", "", 0)]);
    let warns = unknown_warnings();
    assert_eq!(warns.len(), 3, "{warns:?}");
    for key in ["mystery", "extra"] {
        assert!(
            warns[1..]
                .iter()
                .any(|message| message == &format!("node odd has an unknown prop {key}")),
            "missing an unknown-prop warning for {key} in {warns:?}"
        );
    }
}

#[gpui::test]
fn set_props_tells_the_view_the_merged_props(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, guest, _created, told, shown) = setup(&mut app);
    let _window = open_and_draw(&mut app, &shown);
    assert!(told.borrow().is_empty());
    app.update(|cx| {
        host.set_props(cx, "alpha", json!({"tint": "warning", "count": 3}));
        // Props stay with the node when it is moved, hidden, or shown.
        guest.move_node(cx, "alpha", "root", "children", 0);
        guest.set_hidden(cx, "alpha", true);
        guest.set_hidden(cx, "alpha", false);
        // A key set to null is removed: its default is used again.
        host.set_props(cx, "alpha", json!({"tint": null}));
        // Any extension may set the props of any node.
        guest.set_props(cx, "beta", json!({"title": "Beta"}));
        // A component registered without props keeps them in the model; its view is not told.
        host.set_props(cx, "plain", json!({"title": "P"}));
        // A node not in the tree is refused.
        host.set_props(cx, "ghost", json!({"title": "G"}));
        assert_eq!(shell::user_layer_len(cx), 7);
    });
    assert_eq!(
        *told.borrow(),
        [
            ("alpha".to_string(), CardProps::new("Alpha", "warning", 3)),
            ("alpha".to_string(), CardProps::new("Alpha", "", 3)),
            ("beta".to_string(), CardProps::new("Beta", "", 0)),
        ]
    );
}

#[gpui::test]
fn a_reset_tells_each_changed_view_its_default_props(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let (host, guest, _created, told, shown) = setup(&mut app);
    let window = open_and_draw(&mut app, &shown);
    app.update(|cx| {
        host.set_props(cx, "alpha", json!({"tint": "warning"}));
        guest.set_props(cx, "beta", json!({"title": "Beta"}));
    });
    gpui::AnyWindowHandle::from(window)
        .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
        .unwrap();
    // Every node has its default props again; the views that changed are told.
    assert_eq!(
        told_props(&told, "alpha"),
        [
            CardProps::new("Alpha", "warning", 0),
            CardProps::new("Alpha", "accent", 0),
        ]
    );
    assert_eq!(
        told_props(&told, "beta"),
        [CardProps::new("Beta", "", 0), CardProps::default()]
    );
    // A view whose props did not change is not told again.
    assert!(told_props(&told, "bad").is_empty());
    assert!(told_props(&told, "odd").is_empty());
    assert!(told_props(&told, "plain").is_empty());
    assert_eq!(told.borrow().len(), 4);
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
}

#[gpui::test]
fn props_that_do_not_encode_as_an_object_are_an_error(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let created = Created::default();
    let told = Told::default();
    let shown = Shown::default();
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = created.clone();
        let told = told.clone();
        let shown = shown.clone();
        host.register_with_props::<CardProps, Card>(
            cx,
            "tour.card",
            &["body"],
            move |props, node, slots, _, _| {
                created.borrow_mut().push((node.to_string(), props));
                Card {
                    node: node.to_string(),
                    slots,
                    shown: shown.clone(),
                    told: told.clone(),
                }
            },
        );
        host.place_root(cx, "root", "yard.row");
        host.place_with_props(
            cx,
            "alpha",
            "tour.card",
            "root",
            "children",
            json!({"title": "Alpha"}),
        );
        // An extension bug: the props do not encode as a JSON object.
        host.place_with_props(cx, "badplace", "tour.card", "root", "children", vec![1, 2]);
        host
    });
    let window = open_and_draw(&mut app, &shown);
    // The node is not placed.
    assert_eq!(*shown.borrow(), ["alpha"]);
    assert_eq!(created_props(&created, "badplace"), None);
    gpui::AnyWindowHandle::from(window)
        .update(&mut app, |_, window, cx| {
            host.add_with_props(
                window,
                cx,
                "badadd",
                "tour.card",
                "root",
                "children",
                json!("text"),
            );
        })
        .unwrap();
    app.update(|cx| host.set_props(cx, "alpha", json!(7)));
    assert_eq!(*shown.borrow(), ["alpha"]);
    assert_eq!(created_props(&created, "badadd"), None);
    assert!(told.borrow().is_empty());
    assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    // Nothing changed: a good set_props still works afterwards.
    app.update(|cx| host.set_props(cx, "alpha", json!({"count": 9})));
    assert_eq!(told_props(&told, "alpha"), [CardProps::new("Alpha", "", 9)]);
    let errors: Vec<String> = warnings()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect();
    assert_eq!(errors.len(), 3, "{errors:?}");
}

#[gpui::test]
fn a_saved_set_props_is_restored_at_the_next_start(cx: &mut TestAppContext) {
    let _guard = lock();
    let temp = TempDir::new();
    let layout_path = temp.path().join("data/layout.json");
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, _guest, created, _told, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                host.set_props(cx, "beta", json!({"title": "Beta", "count": 4}));
                host.add_with_props(
                    window,
                    cx,
                    "added",
                    "tour.card",
                    "root",
                    "children",
                    json!({"title": "Added", "tint": "success"}),
                );
                host.add(window, cx, "addedplain", "tour.card", "root", "children");
            })
            .unwrap();
        assert_eq!(
            created_props(&created, "added"),
            Some(CardProps::new("Added", "success", 0))
        );
        // `add` places a node with no props.
        assert_eq!(
            created_props(&created, "addedplain"),
            Some(CardProps::default())
        );
        let saved = fs::read_to_string(&layout_path).unwrap();
        let ops = decode(&saved).unwrap();
        assert_eq!(ops.len(), 3);
        match &ops[0] {
            Operation::SetProps { node, props } => {
                assert_eq!(node.as_str(), "beta");
                assert_eq!(
                    serde_json::Value::Object(props.clone()),
                    json!({"title": "Beta", "count": 4})
                );
            }
            other => panic!("expected a props operation, got {other:?}"),
        }
        match &ops[1] {
            Operation::Add { node, .. } => {
                assert_eq!(node.id.as_str(), "added");
                assert_eq!(
                    serde_json::Value::Object(node.props.clone()),
                    json!({"title": "Added", "tint": "success"})
                );
            }
            other => panic!("expected an add, got {other:?}"),
        }
        match &ops[2] {
            Operation::Add { node, .. } => {
                assert_eq!(node.id.as_str(), "addedplain");
                assert!(node.props.is_empty());
            }
            other => panic!("expected an add, got {other:?}"),
        }
    }
    // At the next start the props operation is applied and read into the view.
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (_host, _guest, created, _told, shown) = setup(&mut app);
        let _window = open_and_draw(&mut app, &shown);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 3);
        assert_eq!(
            created_props(&created, "beta"),
            Some(CardProps::new("Beta", "", 4))
        );
        assert_eq!(
            created_props(&created, "added"),
            Some(CardProps::new("Added", "success", 0))
        );
        assert_eq!(
            *shown.borrow(),
            [
                "alpha",
                "beta",
                "bad",
                "odd",
                "plain",
                "added",
                "addedplain"
            ]
        );
    }
    // A saved set-props whose node is not in the tree is skipped, as feature 12 says.
    fs::write(
        &layout_path,
        r#"{"version": 1, "operations": [
            {"op": "props", "node": "beta", "props": {"title": "Back"}},
            {"op": "props", "node": "ghost", "props": {"title": "Gone"}},
            {"op": "props", "node": "plain", "props": {"note": "kept"}}
        ]}"#,
    )
    .unwrap();
    {
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, _guest, created, _told, shown) = setup(&mut app);
        let _window = open_and_draw(&mut app, &shown);
        // The ghost's operation is skipped, not recorded.
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 2);
        assert_eq!(
            created_props(&created, "beta"),
            Some(CardProps::new("Back", "", 0))
        );
        // The next save drops the skipped operation.
        app.update(|cx| host.set_hidden(cx, "alpha", true));
        let ops = decode(&fs::read_to_string(&layout_path).unwrap()).unwrap();
        assert_eq!(
            ops,
            [
                Operation::SetProps {
                    node: id("beta"),
                    props: object(json!({"title": "Back"})),
                },
                Operation::SetProps {
                    node: id("plain"),
                    props: object(json!({"note": "kept"})),
                },
                Operation::SetHidden {
                    node: id("alpha"),
                    hidden: true,
                },
            ]
        );
    }
}

#[gpui::test]
fn place_root_with_props_sets_the_root_nodes_props(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let created = Created::default();
    let shown = Shown::default();
    app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = created.clone();
        let shown = shown.clone();
        host.register_with_props::<CardProps, Card>(
            cx,
            "tour.card",
            &["body"],
            move |props, node, slots, _, _| {
                created.borrow_mut().push((node.to_string(), props));
                Card {
                    node: node.to_string(),
                    slots,
                    shown: shown.clone(),
                    told: Told::default(),
                }
            },
        );
        host.place_root_with_props(
            cx,
            "root",
            "tour.card",
            json!({"title": "Root", "count": 5}),
        );
        host
    });
    let _window = open_and_draw(&mut app, &shown);
    assert_eq!(
        created_props(&created, "root"),
        Some(CardProps::new("Root", "", 5))
    );
    assert_eq!(*shown.borrow(), ["root"]);
}
