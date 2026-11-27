//! Acceptance tests for roadmap #14 `extension-toggle`: keeping a turned-off
//! extension's user layer and saved named layout across simulated runs,
//! dropping unapplicable operations when every extension is on, and dropping
//! kept operations on `reset_layout`.

use gpui::{
    App, AppContext, Global, IntoElement, Menu, MenuItem, ParentElement, Styled, TestAppContext,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_app::{Application, Init, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;
use yard_shell::{Extension, NodeInfo, Quit, Shell, Slots};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-app-extension-toggle-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_app() -> Application {
    Application {
        product: Product {
            name: "Toggle Test",
            version: "0.1.0",
            identifier: "test.yard.toggle",
        },
        url_scheme: "toggletest",
        update_url: "",
        window_icon: None,
    }
}

fn test_menus(product: &Product) -> Vec<Menu> {
    vec![Menu::new(product.name).items(vec![MenuItem::action(
        format!("Quit {}", product.name),
        Quit,
    )])]
}

struct CardView {
    slots: Slots,
}

impl gpui::Render for CardView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().children(self.slots.views("body"))
    }
}

struct NoteView;

impl gpui::Render for NoteView {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

#[derive(Default)]
struct Handles {
    e_inits: usize,
    f_inits: usize,
    f: Option<Extension>,
}

impl Global for Handles {}

fn init_f(cx: &mut App, ext: &Extension) {
    let handles = cx.default_global::<Handles>();
    handles.f_inits += 1;
    handles.f = Some(ext.clone());
    ext.register(cx, "f.note", &[], |_, _, _, cx| cx.new(|_| NoteView).into());
    ext.place_root(cx, "root", "yard.row");
    ext.place(cx, "note", "f.note", "root", "children");
    ext.place(cx, "extra", "f.note", "root", "children");
}

fn init_e(cx: &mut App, ext: &Extension) {
    cx.default_global::<Handles>().e_inits += 1;
    ext.register(cx, "e.card", &["body"], |_, slots, _, cx| {
        cx.new(|_| CardView { slots }).into()
    });
    ext.place(cx, "card", "e.card", "root", "children");
    ext.place(cx, "side", "e.card", "root", "children");
    ext.named_layout(cx, "e.focus", |layout| {
        layout.move_node("card", "root", "children", 0);
    });
}

fn extensions() -> &'static [(&'static str, Init)] {
    &[("f", init_f), ("e", init_e)]
}

fn write_settings(paths: &Paths, value: &Value) {
    let file = paths.settings_file();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()))
}

fn start_run(app: &mut TestAppContext, paths: &Paths) -> gpui::WindowHandle<Shell> {
    let product_app = test_app();
    let open_urls = RefCell::new(OpenUrls::default());
    let window = app.update(|cx| {
        yard_app::init(cx, product_app, paths, extensions(), test_menus, &open_urls);
        let session = yard_shell::saved_session(cx, &paths.session_file());
        yard_shell::track_session(cx, paths.session_file());
        match session {
            Some(windows) => {
                let saved = &windows[0];
                cx.open_window(
                    gpui::WindowOptions {
                        window_bounds: saved.bounds,
                        display_id: saved.display,
                        ..Default::default()
                    },
                    |window, cx| yard_shell::new_session_shell(window, cx, saved),
                )
                .unwrap()
            }
            None => cx
                .open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
                .unwrap(),
        }
    });
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
    window
}

fn settle(app: &mut TestAppContext) {
    app.run_until_parked();
    app.executor().advance_clock(Duration::from_millis(200));
    app.run_until_parked();
}

fn f_handle(app: &TestAppContext) -> Extension {
    app.read(|cx| cx.global::<Handles>().f.clone().expect("f initialized"))
}

fn init_counts(app: &TestAppContext) -> (usize, usize) {
    app.read(|cx| {
        let handles = cx.global::<Handles>();
        (handles.f_inits, handles.e_inits)
    })
}

fn parent_and_slot(nodes: &[NodeInfo], id: &str) -> Option<(String, String)> {
    let index = nodes.iter().position(|node| node.id.as_str() == id)?;
    let depth = nodes[index].depth;
    let slot = nodes[index].slot.as_ref()?.to_string();
    let parent = nodes[..index]
        .iter()
        .rev()
        .find(|node| node.depth + 1 == depth)?
        .id
        .to_string();
    Some((parent, slot))
}

fn children_in_slot(nodes: &[NodeInfo], parent: &str, slot: &str) -> Vec<String> {
    let Some(start) = nodes.iter().position(|node| node.id.as_str() == parent) else {
        return Vec::new();
    };
    let child_depth = nodes[start].depth + 1;
    let mut children = Vec::new();
    for node in &nodes[start + 1..] {
        if node.depth < child_depth {
            break;
        }
        if node.depth == child_depth && node.slot.as_deref() == Some(slot) {
            children.push(node.id.to_string());
        }
    }
    children
}

// Contract point 3:
// - run 1 (e and f on): move `note` into `card`'s slot, hide `side`; quit.
// - run 2 (e off): hide `extra`; quit. Both `session.json` and `layout.json`
//   hold every operation of run 1 in order, followed by the run 2 edit.
// - run 3 (e on again): `note` is in `card`'s slot, `side` is hidden, and
//   `extra` is hidden.
#[gpui::test]
fn turning_an_extension_off_keeps_its_user_layer_across_runs(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let paths = Paths::under(temp.path());

    // Run 1: both `f` and `e` are on.
    let run1_ops = {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        assert_eq!(init_counts(&app), (1, 1));
        let f = f_handle(&app);
        app.update(|cx| {
            f.move_node(cx, "note", "card", "body", 0);
            f.set_hidden(cx, "side", true);
        });
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert_eq!(
                parent_and_slot(&nodes, "note"),
                Some(("card".into(), "body".into()))
            );
            assert!(yard_shell::is_hidden(cx, "side"));
            assert!(!yard_shell::is_hidden(cx, "extra"));
        });
        app.quit();

        let layout = read_json(&paths.layout_file());
        let ops = layout["operations"]
            .as_array()
            .expect("layout.json operations")
            .clone();
        assert_eq!(ops.len(), 2, "run 1 wrote both edits: {layout}");
        ops
    };

    // Run 2: `e` is turned off; make one more edit on `f`'s node `extra`.
    write_settings(&paths, &json!({ "extensions": { "e": false } }));
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        assert_eq!(
            init_counts(&app),
            (1, 0),
            "`e` must not run when turned off in settings.json"
        );
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert!(
                !nodes.iter().any(|node| node.extension.as_str() == "e"),
                "no nodes of `e` while `e` is off: {nodes:?}"
            );
        });
        let f = f_handle(&app);
        app.update(|cx| {
            f.set_hidden(cx, "extra", true);
        });
        assert!(app.read(|cx| yard_shell::is_hidden(cx, "extra")));
        app.quit();
    }

    let mut expected_ops = run1_ops;
    expected_ops.push(json!({ "op": "hide", "node": "extra" }));

    let layout = read_json(&paths.layout_file());
    assert_eq!(
        layout["operations"],
        Value::Array(expected_ops.clone()),
        "layout.json must keep run 1 operations in order followed by run 2 edit"
    );
    let session = read_json(&paths.session_file());
    assert_eq!(
        session["windows"][0]["layers"]["yard.default"],
        Value::Array(expected_ops),
        "session.json must keep run 1 operations in order followed by run 2 edit"
    );

    // Run 3: `e` is on again.
    write_settings(&paths, &json!({}));
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        assert_eq!(init_counts(&app), (1, 1));
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert_eq!(
                parent_and_slot(&nodes, "note"),
                Some(("card".into(), "body".into())),
                "`note` returns to `card`'s `body` slot: {nodes:?}"
            );
            assert!(
                yard_shell::is_hidden(cx, "side"),
                "`side` stays hidden when `e` returns"
            );
            assert!(
                yard_shell::is_hidden(cx, "extra"),
                "`extra` edit from run 2 stays applied"
            );
            assert!(!yard_shell::is_hidden(cx, "card"));
        });
        app.quit();
    }
}

// Contract point 4:
// With every extension on, an operation that cannot apply (naming a node no
// extension places) is dropped on save, as today.
#[gpui::test]
fn skipped_operation_is_dropped_when_every_extension_is_on(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let paths = Paths::under(temp.path());

    // Run 1: produce valid session.json and layout.json with one edit.
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        let f = f_handle(&app);
        app.update(|cx| f.set_hidden(cx, "note", true));
        app.quit();
    }

    // Prepend an unapplicable operation naming a node no extension places.
    let ghost = json!({ "op": "hide", "node": "ghost" });
    let mut layout = read_json(&paths.layout_file());
    layout["operations"]
        .as_array_mut()
        .unwrap()
        .insert(0, ghost.clone());
    fs::write(
        paths.layout_file(),
        serde_json::to_string_pretty(&layout).unwrap(),
    )
    .unwrap();

    let mut session = read_json(&paths.session_file());
    session["windows"][0]["layers"]["yard.default"]
        .as_array_mut()
        .unwrap()
        .insert(0, ghost);
    fs::write(
        paths.session_file(),
        serde_json::to_string_pretty(&session).unwrap(),
    )
    .unwrap();

    // Run 2: every extension is on; save after one more edit.
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        assert_eq!(init_counts(&app), (1, 1));
        assert!(app.read(|cx| yard_shell::is_hidden(cx, "note")));
        let f = f_handle(&app);
        app.update(|cx| f.set_hidden(cx, "extra", true));
        app.quit();
    }

    let expected = json!([
        { "op": "hide", "node": "note" },
        { "op": "hide", "node": "extra" }
    ]);
    assert_eq!(
        read_json(&paths.layout_file())["operations"],
        expected,
        "layout.json drops unapplicable operation when every extension is on"
    );
    assert_eq!(
        read_json(&paths.session_file())["windows"][0]["layers"]["yard.default"],
        expected,
        "session.json drops unapplicable operation when every extension is on"
    );
}

// Contract point 5:
// A window whose saved named layout is `e.focus`, with `e` off: shows the
// default layout, and `session.json` still names `e.focus` after the save;
// turning `e` on shows `e.focus` again.
#[gpui::test]
fn saved_named_layout_of_turned_off_extension_is_kept_in_session(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let paths = Paths::under(temp.path());

    // Run 1: both extensions on; switch to `e.focus` and quit.
    {
        let mut app = cx.new_app();
        let window = start_run(&mut app, &paths);
        let f = f_handle(&app);
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                f.show_layout(window, cx, "e.focus");
                assert_eq!(yard_shell::shown_layout(cx, window).as_ref(), "e.focus");
            })
            .unwrap();
        app.read(|cx| {
            let children = children_in_slot(&yard_shell::inspect(cx), "root", "children");
            assert_eq!(children.first().map(String::as_str), Some("card"));
        });
        app.quit();
        assert_eq!(
            read_json(&paths.session_file())["windows"][0]["layout"],
            "e.focus"
        );
    }

    // Run 2: `e` is off; the window shows `yard.default`, and `session.json`
    // still names `e.focus` after the save.
    write_settings(&paths, &json!({ "extensions": { "e": false } }));
    {
        let mut app = cx.new_app();
        let window = start_run(&mut app, &paths);
        assert_eq!(
            init_counts(&app),
            (1, 0),
            "`e` must not run when turned off"
        );
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                assert_eq!(
                    yard_shell::shown_layout(cx, window).as_ref(),
                    "yard.default",
                    "with `e` off, the window falls back to the default layout"
                );
            })
            .unwrap();
        settle(&mut app);
        app.quit();
        assert_eq!(
            read_json(&paths.session_file())["windows"][0]["layout"],
            "e.focus",
            "session.json must keep `e.focus` while `e` is off"
        );
    }

    // Run 3: `e` is on again; the window shows `e.focus`.
    write_settings(&paths, &json!({}));
    {
        let mut app = cx.new_app();
        let window = start_run(&mut app, &paths);
        assert_eq!(init_counts(&app), (1, 1));
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                assert_eq!(
                    yard_shell::shown_layout(cx, window).as_ref(),
                    "e.focus",
                    "turning `e` back on restores `e.focus`"
                );
            })
            .unwrap();
        app.read(|cx| {
            let children = children_in_slot(&yard_shell::inspect(cx), "root", "children");
            assert_eq!(children.first().map(String::as_str), Some("card"));
        });
        app.quit();
    }
}

// Contract point 6:
// `reset_layout` in a run with `e` off drops the kept operations: after it,
// the saved layer has none of them.
#[gpui::test]
fn reset_layout_while_extension_is_off_drops_kept_operations(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let paths = Paths::under(temp.path());

    // Run 1: edits that all depend on `e` (so run 2 has only kept operations).
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        let f = f_handle(&app);
        app.update(|cx| {
            f.move_node(cx, "note", "card", "body", 0);
            f.set_hidden(cx, "side", true);
        });
        app.quit();
        assert_eq!(
            read_json(&paths.session_file())["windows"][0]["layers"]["yard.default"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    // Run 2: `e` is off; call `reset_layout` and quit.
    write_settings(&paths, &json!({ "extensions": { "e": false } }));
    {
        let mut app = cx.new_app();
        let window = start_run(&mut app, &paths);
        assert_eq!(
            init_counts(&app),
            (1, 0),
            "`e` must not run when turned off"
        );
        let f = f_handle(&app);
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                f.reset_layout(window, cx);
            })
            .unwrap();
        app.quit();
    }

    assert_eq!(
        read_json(&paths.session_file())["windows"][0]["layers"]["yard.default"],
        json!([]),
        "session.json must have no kept operations after reset_layout"
    );
    assert_eq!(
        read_json(&paths.layout_file())["operations"],
        json!([]),
        "layout.json must have no kept operations after reset_layout"
    );

    // Run 3: turning `e` back on shows the default tree without the reset edits.
    write_settings(&paths, &json!({}));
    {
        let mut app = cx.new_app();
        let _window = start_run(&mut app, &paths);
        assert_eq!(init_counts(&app), (1, 1));
        app.read(|cx| {
            let nodes = yard_shell::inspect(cx);
            assert_eq!(
                parent_and_slot(&nodes, "note"),
                Some(("root".into(), "children".into())),
                "`note` stays in its default slot after reset_layout"
            );
            assert!(!yard_shell::is_hidden(cx, "side"));
        });
        app.quit();
    }
}
