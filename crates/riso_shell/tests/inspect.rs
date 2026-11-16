#![cfg(feature = "dev")]

use gpui::{
    AppContext, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    TestAppContext, px,
};
use riso_shell::dev as shell;
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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-inspect-{}-{}-{nanos:x}",
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

// Every test in this file shares one process-wide diagnostic logger and runs
// under one lock, so a test never sees another test's records.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS
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
    RECORDS.lock().unwrap().clear();
    guard
}

fn records() -> Vec<(log::Level, String)> {
    RECORDS.lock().unwrap().clone()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// The model the item slot of these tests is over: a list of names.
struct Names(Vec<SharedString>);

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

// A card fills the zone its container gives it and is found as `CARD-<node>`.
struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

// A view that draws the children of each of its component's slots, in order.
struct Container {
    slots: shell::Slots,
    slot_names: &'static [&'static str],
}

impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().flex().flex_col().children(
            self.slot_names
                .iter()
                .flat_map(|name| self.slots.views(name))
                .map(|view| gpui::div().flex_1().min_h_0().child(view)),
        )
    }
}

// `host` registers the components of these tests: `tour.card` is a leaf,
// `tour.frame` has the two slots `first` and `second`, and `tour.names` has
// the slot `children` and gives the `Names` model to its descendants.
fn extension(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |node, _, _, cx| {
            cx.new(|_| Card {
                node: node.to_string(),
            })
            .into()
        });
        host.register(cx, "tour.frame", &["first", "second"], |_, slots, _, cx| {
            cx.new(|_| Container {
                slots,
                slot_names: &["first", "second"],
            })
            .into()
        });
        host.register(cx, "tour.names", &["children"], |_, slots, _, cx| {
            cx.new(|_| Container {
                slots,
                slot_names: &["children"],
            })
            .into()
        });
        host.provide::<Names>(cx, "tour.names", |_, cx| {
            cx.new(|_| Names(vec!["ada".into(), "bob".into()]))
        });
        host
    })
}

// Draw without forcing a refresh: operations must schedule it themselves.
fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

// An operation that needs the window runs inside it.
fn window_update(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    f: impl FnOnce(&mut gpui::Window, &mut gpui::App),
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| f(window, cx))
        .unwrap();
}

// One `NodeInfo` as text: id, depth, slot, component, extension, props,
// hidden flag, and origin, so the whole list can be compared at once.
fn row(info: &shell::NodeInfo) -> String {
    let origin = match &info.origin {
        shell::Origin::Default => "default".to_string(),
        shell::Origin::NamedLayout(name) => format!("named {name}"),
        shell::Origin::UserLayer => "user".to_string(),
        shell::Origin::Copy(key) => format!("copy {key}"),
    };
    format!(
        "{} d{} {} {} {} {} {} {}",
        info.id,
        info.depth,
        info.slot.as_deref().unwrap_or("-"),
        info.component,
        info.extension,
        info.props,
        if info.hidden { "hidden" } else { "shown" },
        origin,
    )
}

fn rows(infos: &[shell::NodeInfo]) -> Vec<String> {
    infos.iter().map(row).collect()
}

fn ids(infos: &[shell::NodeInfo]) -> Vec<String> {
    infos.iter().map(|info| info.id.to_string()).collect()
}

// Every `observe_layout` call pushes the ids `inspect` then lists.
type Calls = Rc<RefCell<Vec<Vec<String>>>>;

fn observed(app: &mut TestAppContext, calls: &Calls) -> gpui::Subscription {
    let calls = calls.clone();
    app.update(|cx| {
        shell::observe_layout(cx, move |cx| {
            calls.borrow_mut().push(ids(&shell::inspect(cx)));
        })
    })
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn selected(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|_, cx| shell::selected_tab(cx, "tabs").map(|node| node.to_string()))
}

// The handle's hitbox is centered on the child's edge, so the edge itself is
// inside it.
fn handle(visual: &mut gpui::VisualTestContext, before: &'static str) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(before).expect("the child draws");
    gpui::point(bounds.right(), bounds.center().y)
}

// `inspect` lists the tree depth first: a node's slots in their declared
// order, each slot's children in their order, a child before the parent's
// next sibling, with depth, slot, component, extension, props, and hidden.
#[gpui::test]
fn inspect_lists_the_tree_depth_first_in_slot_and_child_order(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root_with_props(cx, "root", "tour.frame", json!({"note": "top"}));
        host.place(cx, "a", "tour.card", "root", "first");
        host.place_with_props(cx, "b", "tour.card", "root", "first", json!({"n": 2}));
        host.place(cx, "nest", "riso.row", "root", "second");
        host.place(cx, "c", "tour.card", "nest", "children");
        host.place(cx, "d", "tour.card", "nest", "children");
        host.place(cx, "tail", "tour.card", "root", "second");
        host.set_hidden(cx, "b", true);
    });
    let _window = open_and_draw(&mut app);
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx))),
        [
            r#"root d0 - tour.frame tour {"note":"top"} shown default"#,
            r#"a d1 first tour.card tour null shown default"#,
            r#"b d1 first tour.card tour {"n":2} hidden default"#,
            r#"nest d1 second riso.row tour null shown default"#,
            r#"c d2 children tour.card tour null shown default"#,
            r#"d d2 children tour.card tour null shown default"#,
            r#"tail d1 second tour.card tour null shown default"#,
        ]
    );
    // `hidden` is the node's own flag, and the list follows the tree as it
    // changes: `c` still has its place, hidden now.
    app.update(|cx| host.set_hidden(cx, "c", true));
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx)))[4],
        r#"c d2 children tour.card tour null hidden default"#
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// `origin` says what put each node in the tree: a node placed in the default
// layout is `default`, a node an operation added is `user`, a copy of an
// item slot's template is `copy <key>` in its slot, and a node the shown
// named layout added is `named <name>`.
#[gpui::test]
fn inspect_marks_what_put_each_node_in_the_tree(cx: &mut TestAppContext) {
    let _guard = lock();
    let mut app = cx.new_app();
    let host = extension(&mut app);
    app.update(|cx| {
        host.place_root(cx, "root", "tour.names");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "row", "riso.row", "root", "children");
        host.item_slot::<Names>(cx, "row", "children");
        host.place(cx, "name", "tour.card", "row", "children");
        host.named_layout(cx, "tour.extra", |layout| {
            layout.add("extra", "tour.card", "root", "children");
        });
    });
    let window = open_and_draw(&mut app);
    // `name` is the item slot's one template: it is listed as the node it
    // is, and each copy of it is listed in the slot, in the items' order.
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx))),
        [
            r#"root d0 - tour.names tour null shown default"#,
            r#"a d1 children tour.card tour null shown default"#,
            r#"row d1 children riso.row tour null shown default"#,
            r#"name d2 children tour.card tour null shown default"#,
            r#"name@ada d2 children tour.card tour null shown copy ada"#,
            r#"name@bob d2 children tour.card tour null shown copy bob"#,
        ]
    );

    // An operation's add is the shown layout's user layer.
    window_update(window, &mut app, |window, cx| {
        host.add(window, cx, "added", "tour.card", "root", "children")
    });
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx))),
        [
            r#"root d0 - tour.names tour null shown default"#,
            r#"a d1 children tour.card tour null shown default"#,
            r#"row d1 children riso.row tour null shown default"#,
            r#"name d2 children tour.card tour null shown default"#,
            r#"name@ada d2 children tour.card tour null shown copy ada"#,
            r#"name@bob d2 children tour.card tour null shown copy bob"#,
            r#"added d1 children tour.card tour null shown user"#,
        ]
    );

    // On `tour.extra`, `extra` is the named layout's own; `added` stays in
    // the default layout's user layer and is not there.
    window_update(window, &mut app, |window, cx| {
        host.show_layout(window, cx, "tour.extra")
    });
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx))),
        [
            r#"root d0 - tour.names tour null shown default"#,
            r#"a d1 children tour.card tour null shown default"#,
            r#"row d1 children riso.row tour null shown default"#,
            r#"name d2 children tour.card tour null shown default"#,
            r#"name@ada d2 children tour.card tour null shown copy ada"#,
            r#"name@bob d2 children tour.card tour null shown copy bob"#,
            r#"extra d1 children tour.card tour null shown named tour.extra"#,
        ]
    );

    // An operation while `tour.extra` is shown is in its user layer too.
    window_update(window, &mut app, |window, cx| {
        host.add(window, cx, "added2", "tour.card", "root", "children")
    });
    assert_eq!(
        rows(&app.update(|cx| shell::inspect(cx)))[7],
        r#"added2 d1 children tour.card tour null shown user"#
    );
    assert!(errors().is_empty(), "{:?}", errors());
}

// `observe_layout` is called once per change — an operation, a reset, a
// restore, a switch of named layout, and items changing a slot's copies —
// after the change is complete, and not for a change of view state.
#[gpui::test]
fn observe_layout_is_called_once_after_each_change_and_not_for_view_state(cx: &mut TestAppContext) {
    let _guard = lock();
    {
        // The observer is registered before the window opens: restoring the
        // saved hide of `t1` at the open is one change and one call.
        let temp = TempDir::new();
        let layout_path = temp.path().join("layout.json");
        fs::write(
            &layout_path,
            r#"{"version":1,"operations":[{"op":"hide","node":"t1"}]}"#,
        )
        .unwrap();
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path));
        let host = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "riso.row");
            for node in ["a", "b"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.place(cx, "tabs", "riso.tabs", "root", "children");
            for node in ["t1", "t2", "t3"] {
                host.place(cx, node, "tour.card", "tabs", "children");
            }
            host.named_layout(cx, "tour.extra", |layout| {
                layout.add("extra", "tour.card", "root", "children");
            });
        });
        let calls = Calls::default();
        let subscription = observed(&mut app, &calls);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert_eq!(calls.borrow().len(), 1, "the restore is one call");
        assert_eq!(
            calls.borrow()[0],
            ["root", "a", "b", "tabs", "t1", "t2", "t3"]
        );

        // An applied operation is one call, after it: `added` is listed.
        window_update(window, &mut app, |window, cx| {
            host.add(window, cx, "added", "tour.card", "root", "children")
        });
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(
            calls.borrow()[1],
            ["root", "a", "b", "tabs", "t1", "t2", "t3", "added"]
        );
        draw(window, &mut app);

        // Selecting a tab is view state: the selection changed, and no call.
        click(&mut visual, "TAB-t3");
        assert_eq!(selected(&mut visual).as_deref(), Some("t3"));
        assert_eq!(calls.borrow().len(), 2);

        // So are the shares while a split's handle is being dragged: no call
        // while the button is down. The mouse up applies the drag's
        // set-props, which is an operation: one call.
        let at = handle(&mut visual, "CARD-a");
        let to = gpui::point(at.x + px(100.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
        assert_eq!(calls.borrow().len(), 2);
        visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
        assert_eq!(calls.borrow().len(), 3);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 3);

        // Zooming is view state: the zoom took, and no call.
        app.update(|cx| host.set_zoomed(cx, "a"));
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));
        assert_eq!(calls.borrow().len(), 3);

        // A reset is one call, after it: `added` is gone again.
        window_update(window, &mut app, |window, cx| host.reset_layout(window, cx));
        assert_eq!(calls.borrow().len(), 4);
        assert_eq!(
            calls.borrow()[3],
            ["root", "a", "b", "tabs", "t1", "t2", "t3"]
        );

        // A switch of named layout is one call, after it: `extra` is listed.
        window_update(window, &mut app, |window, cx| {
            host.show_layout(window, cx, "tour.extra")
        });
        assert_eq!(calls.borrow().len(), 5);
        assert_eq!(
            calls.borrow()[4],
            ["root", "a", "b", "tabs", "t1", "t2", "t3", "extra"]
        );

        // Dropping the subscription stops it.
        drop(subscription);
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert_eq!(calls.borrow().len(), 5);
    }

    // Items that change an item slot's copies are one call, after the
    // change: `name@dee` is already listed.
    {
        let mut app = cx.new_app();
        let host = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "tour.names");
            host.place(cx, "row", "riso.row", "root", "children");
            host.item_slot::<Names>(cx, "row", "children");
            host.place(cx, "name", "tour.card", "row", "children");
        });
        let calls = Calls::default();
        let _subscription = observed(&mut app, &calls);
        let window = open_and_draw(&mut app);
        let before = calls.borrow().len();
        let names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        app.update(|cx| {
            names.update(cx, |model, cx| {
                model.0.push("dee".into());
                cx.notify();
            });
        });
        draw(window, &mut app);
        assert_eq!(calls.borrow().len(), before + 1);
        assert_eq!(
            calls.borrow()[before],
            ["root", "row", "name", "name@ada", "name@bob", "name@dee"]
        );
    }
    assert!(errors().is_empty(), "{:?}", errors());
}
