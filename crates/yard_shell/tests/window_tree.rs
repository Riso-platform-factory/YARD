use gpui::{
    AppContext, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    TestAppContext, px,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_layout::{Operation, decode};
use yard_shell as shell;

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs, with the records' groups.
const PROBE: &str = "window_tree_probe";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-window-tree-{}-{}-{nanos:x}",
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
// under one lock, so a test never sees another test's records, and a test
// that captures the lifecycle trace never runs beside another that reaches
// the same lines.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        RECORDS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((record.level(), record.args().to_string()));
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            log::Level::Info => tracing::info!(target: PROBE, "{}", record.args()),
            log::Level::Debug => tracing::debug!(target: PROBE, "{}", record.args()),
            log::Level::Trace => tracing::trace!(target: PROBE, "{}", record.args()),
        }
    }
    fn flush(&self) {}
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    // A panicked test poisons the shared state; recover so the next test
    // still reports its own verdict.
    let guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    guard
}

fn records() -> Vec<(log::Level, String)> {
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
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
        captured
            .spans
            .insert(id, format!("{}{}", attributes.metadata().name(), fields.0));
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let target = event.metadata().target();
        if !(target.starts_with("yard_shell")
            || target.starts_with("yard_layout")
            || target == PROBE)
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
        captured.lines.push(line);
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

impl Trace {
    fn mark(&self) -> usize {
        self.0.lock().unwrap().lines.len()
    }
    fn since(&self, mark: usize) -> Vec<String> {
        self.0.lock().unwrap().lines[mark..].to_vec()
    }
}

// Every test runs under the one lock and the one subscriber, so no thread
// reaches the shell's log lines without them.
fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
    assert!(trace.0.lock().unwrap().entered.is_empty());
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

// A view that draws the children of its `children` slot, in order.
struct Container {
    slots: shell::Slots,
}

impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().flex().flex_col().children(
            self.slots
                .views("children")
                .into_iter()
                .map(|view| gpui::div().flex_1().min_h_0().child(view)),
        )
    }
}

// The view each node got in each window, so views can be told apart across
// windows and checked for being dropped.
type Views = Rc<RefCell<Vec<(u64, String, gpui::WeakEntity<Card>)>>>;

// `host` registers the components of these tests: `tour.card` is a leaf, and
// `tour.names` has the slot `children` and gives the `Names` model to its
// descendants.
fn extension(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, window, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
            });
            created.borrow_mut().push((
                window.window_handle().window_id().as_u64(),
                node.to_string(),
                card.downgrade(),
            ));
            card.into()
        });
        host.register(cx, "tour.names", &["children"], |_, slots, _, cx| {
            cx.new(|_| Container { slots }).into()
        });
        host.provide::<Names>(cx, "tour.names", |_, cx| {
            cx.new(|_| Names(vec!["ada".into(), "bob".into()]))
        });
        host
    });
    (host, views)
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

// The window with the keyboard focus is the one calls act on.
fn activate(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
}

fn window_id(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> u64 {
    window
        .update(app, |_, window, _| {
            window.window_handle().window_id().as_u64()
        })
        .unwrap()
}

// The ids `inspect` lists for the window the calls act on.
fn inspected(app: &mut TestAppContext) -> Vec<String> {
    app.update(|cx| {
        shell::inspect(cx)
            .iter()
            .map(|info| info.id.to_string())
            .collect()
    })
}

fn hidden(app: &mut TestAppContext, node: &str) -> bool {
    app.update(|cx| shell::is_hidden(cx, node))
}

fn view(views: &Views, window: u64, node: &str) -> Option<gpui::Entity<Card>> {
    views
        .borrow()
        .iter()
        .find(|(owner, name, _)| *owner == window && name.as_str() == node)
        .and_then(|(.., view)| view.upgrade())
}

fn shown(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> String {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            shell::shown_layout(cx, window).to_string()
        })
        .unwrap()
}

fn saved_ops(layout_path: &std::path::Path) -> Vec<Operation> {
    decode(&fs::read_to_string(layout_path).unwrap()).unwrap()
}

fn is_hide(operation: &Operation, node: &str) -> bool {
    matches!(operation, Operation::SetHidden { node: id, hidden: true } if id.as_str() == node)
}

// Two windows each get the default layout as it was built at startup, and the
// same node has a different view entity in each window. An operation, a
// reset, a zoom, a switch of named layout, and an item change in one window
// leave the other window's tree, user layer, zoom, shown layout, and item
// copies as they are.
#[gpui::test]
fn two_windows_start_from_the_default_and_change_independently(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "tour.names");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.place(cx, "row", "yard.row", "root", "children");
            host.item_slot::<Names>(cx, "row", "children");
            host.place(cx, "name", "tour.card", "row", "children");
            host.named_layout(cx, "tour.swap", |layout| {
                layout.move_node("c", "root", "children", 0);
                layout.add("extra", "tour.card", "root", "children");
            });
        });
        // `observe_layout` is called after a change of any window's tree.
        let calls = Rc::new(RefCell::new(0usize));
        let _subscription = {
            let calls = calls.clone();
            app.update(|cx| shell::observe_layout(cx, move |_cx| *calls.borrow_mut() += 1))
        };
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);
        let (first_id, second_id) = (window_id(first, &mut app), window_id(second, &mut app));
        assert_ne!(first_id, second_id);
        let default: Vec<String> = ["root", "a", "b", "c", "row", "name", "name@ada", "name@bob"]
            .into_iter()
            .map(|id| id.to_string())
            .collect();

        // Each window starts from the default layout.
        activate(first, &mut app);
        assert_eq!(inspected(&mut app), default);
        activate(second, &mut app);
        assert_eq!(inspected(&mut app), default);

        // The same node has a different view entity in each window.
        let first_a = view(&views, first_id, "a").expect("a has a view in the first window");
        let second_a = view(&views, second_id, "a").expect("a has a view in the second window");
        assert_ne!(first_a.entity_id(), second_a.entity_id());
        let observed = *calls.borrow();

        // A call without a window acts on the window with the focus.
        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert_eq!(*calls.borrow(), observed + 1);
        assert!(hidden(&mut app, "a"));
        activate(second, &mut app);
        assert!(
            !hidden(&mut app, "a"),
            "a hide in the first window reached the second"
        );

        // A call given a window acts on that window, even while the other
        // is active.
        window_update(first, &mut app, |window, cx| {
            host.add(window, cx, "added", "tour.card", "root", "children")
        });
        assert_eq!(*calls.borrow(), observed + 2);
        assert_eq!(
            inspected(&mut app),
            default,
            "an add in the first window reached the second"
        );
        activate(first, &mut app);
        assert!(inspected(&mut app).contains(&"added".to_string()));
        assert!(
            hidden(&mut app, "a"),
            "the first window's layer lost the hide"
        );

        // A reset drops only the first window's user layer — the call is
        // given the window while the second is active; the second window
        // keeps its own layer.
        activate(second, &mut app);
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert_eq!(*calls.borrow(), observed + 3);
        assert!(hidden(&mut app, "b"));
        window_update(first, &mut app, |window, cx| host.reset_layout(window, cx));
        activate(first, &mut app);
        assert_eq!(inspected(&mut app), default, "the reset kept `added`");
        activate(second, &mut app);
        assert!(
            hidden(&mut app, "b"),
            "the first window's reset dropped the second's layer"
        );
        assert_eq!(inspected(&mut app), default);

        // Zoom is each window's own.
        activate(first, &mut app);
        app.update(|cx| host.set_zoomed(cx, "a"));
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));
        activate(second, &mut app);
        assert_eq!(
            app.update(|cx| shell::zoomed(cx)),
            None,
            "the second window zoomed with the first"
        );

        // A switch of named layout changes only the window that switches.
        activate(first, &mut app);
        window_update(first, &mut app, |window, cx| {
            host.show_layout(window, cx, "tour.swap")
        });
        assert_eq!(shown(first, &mut app), "tour.swap");
        let swap: Vec<String> = [
            "root", "c", "a", "b", "row", "name", "name@ada", "name@bob", "extra",
        ]
        .into_iter()
        .map(|id| id.to_string())
        .collect();
        assert_eq!(inspected(&mut app), swap);
        activate(second, &mut app);
        assert_eq!(shown(second, &mut app), "yard.default");
        assert_eq!(
            inspected(&mut app),
            default,
            "the switch changed the second window's layout"
        );
        assert!(
            hidden(&mut app, "b"),
            "the second window's own layer did not survive the first's switch"
        );

        // An item change fills one window's item slot only.
        activate(first, &mut app);
        let first_names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        app.update(|cx| {
            first_names.update(cx, |model, cx| {
                model.0.push("dee".into());
                cx.notify();
            });
        });
        assert!(inspected(&mut app).contains(&"name@dee".to_string()));
        activate(second, &mut app);
        let second_names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        assert_ne!(
            first_names.entity_id(),
            second_names.entity_id(),
            "the windows share one items model"
        );
        assert!(
            !inspected(&mut app).contains(&"name@dee".to_string()),
            "the item change reached the second window"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A call that changes or reads a layout acts on the window with the keyboard
// focus: `set_hidden` while the first window is active hides in the first
// window only, and once the second window takes the focus the same call acts
// on it. `inspect` reads each active window's own tree.
#[gpui::test]
fn a_call_acts_on_the_window_with_the_focus(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
        });
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);

        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert!(hidden(&mut app, "a"));
        activate(second, &mut app);
        assert!(
            !hidden(&mut app, "a"),
            "the first window's hide reached the second window"
        );
        // The focus moved to the second window: calls now act on it.
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert!(hidden(&mut app, "b"));
        assert!(!hidden(&mut app, "a"));

        activate(first, &mut app);
        assert!(hidden(&mut app, "a"));
        assert!(
            !hidden(&mut app, "b"),
            "the second window's hide reached the first"
        );

        // `inspect` reads the active window's tree.
        activate(second, &mut app);
        let nodes = app.update(|cx| shell::inspect(cx));
        let flagged = |id: &str| nodes.iter().find(|node| node.id == id).unwrap().hidden;
        assert!(!flagged("a") && flagged("b"));
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// The window opened first is the main window: `layout.json` and the named
// layouts' files hold its user layers only, and its saved layers are
// restored when it opens. The second window keeps its user layers in
// memory; its operations never reach the files.
#[gpui::test]
fn only_the_main_windows_user_layers_are_saved(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("data").join("layout.json");
        let layouts = temp.path().join("data").join("layouts");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(
            &layout_path,
            r#"{"version":1,"operations":[{"op":"hide","node":"a"}]}"#,
        )
        .unwrap();
        let mut app = cx.new_app();
        let files = layouts.clone();
        app.update(|cx| {
            shell::set_layout_file(cx, layout_path.clone());
            shell::set_named_layout_files(cx, move |name| files.join(format!("{name}.json")));
        });
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.named_layout(cx, "tour.swap", |layout| {
                layout.move_node("c", "root", "children", 0);
            });
        });
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);
        let swap_path = layouts.join("tour.swap.json");

        // The saved layer is the main window's: it restores `hide a`, and
        // the second window starts from the default without it.
        activate(first, &mut app);
        assert!(
            hidden(&mut app, "a"),
            "the main window did not restore the saved layer"
        );
        activate(second, &mut app);
        assert!(
            !hidden(&mut app, "a"),
            "the second window restored the main window's layer"
        );

        // The second window's operations never reach layout.json.
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert!(
            hidden(&mut app, "b"),
            "the second window's own operation did not apply"
        );
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.len(),
            1,
            "the second window's operation was saved: {ops:?}"
        );
        assert!(is_hide(&ops[0], "a"), "{ops:?}");

        // The first window's operations are saved.
        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "c", true));
        let ops = saved_ops(&layout_path);
        assert_eq!(ops.len(), 2, "{ops:?}");
        assert!(is_hide(&ops[0], "a"), "{ops:?}");
        assert!(is_hide(&ops[1], "c"), "{ops:?}");

        // Another operation in the second window: the file still holds only
        // the first window's layer.
        activate(second, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert!(hidden(&mut app, "a"));
        let ops = saved_ops(&layout_path);
        assert_eq!(ops.len(), 2, "the second window's hide was saved: {ops:?}");
        assert!(is_hide(&ops[0], "a"), "{ops:?}");
        assert!(is_hide(&ops[1], "c"), "{ops:?}");

        // The named layouts' files are the main window's too.
        activate(first, &mut app);
        window_update(first, &mut app, |window, cx| {
            host.show_layout(window, cx, "tour.swap")
        });
        app.update(|cx| host.set_hidden(cx, "c", true));
        let ops = saved_ops(&swap_path);
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(is_hide(&ops[0], "c"), "{ops:?}");

        activate(second, &mut app);
        window_update(second, &mut app, |window, cx| {
            host.show_layout(window, cx, "tour.swap")
        });
        app.update(|cx| host.set_hidden(cx, "a", true));
        let ops = saved_ops(&swap_path);
        assert_eq!(
            ops.len(),
            1,
            "the second window's named-layout layer was saved: {ops:?}"
        );
        assert!(is_hide(&ops[0], "c"), "{ops:?}");

        // The second window's own layer is still kept, in memory.
        assert!(
            hidden(&mut app, "a"),
            "the second window's in-memory layer lost the hide"
        );
        let ops = saved_ops(&layout_path);
        assert_eq!(ops.len(), 2, "{ops:?}");
        assert!(is_hide(&ops[0], "a"), "{ops:?}");
        assert!(is_hide(&ops[1], "c"), "{ops:?}");
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// When the main window closes and another stays, no window becomes the
// main one: `layout.json` keeps the main window's last state, and a later
// operation in the other window writes nothing.
#[gpui::test]
fn closing_the_main_window_keeps_its_files_and_writes_nothing_more(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("data").join("layout.json");
        fs::create_dir_all(temp.path().join("data")).unwrap();
        fs::write(&layout_path, r#"{"version":1,"operations":[]}"#).unwrap();
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
        });
        let main = open_and_draw(&mut app);
        let other = open_and_draw(&mut app);

        // The main window's operation is saved.
        activate(main, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        let ops = saved_ops(&layout_path);
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(is_hide(&ops[0], "a"), "{ops:?}");

        // The main window closes; no window becomes the main one.
        main.update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();

        // A later operation in the other window applies in memory and
        // writes nothing: the file keeps the main window's last state.
        activate(other, &mut app);
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert!(
            hidden(&mut app, "b"),
            "the operation did not reach the surviving window"
        );
        assert!(
            !hidden(&mut app, "a"),
            "the surviving window took the main window's layer"
        );
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.len(),
            1,
            "the surviving window's operation was saved: {ops:?}"
        );
        assert!(is_hide(&ops[0], "a"), "{ops:?}");

        // A window opened later is not the main one either: it starts from
        // the default, and its operations write nothing.
        let third = open_and_draw(&mut app);
        activate(third, &mut app);
        assert!(
            !hidden(&mut app, "a"),
            "a window opened later restored the main window's layer"
        );
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert!(hidden(&mut app, "b"));
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.len(),
            1,
            "a window opened later had its operation saved: {ops:?}"
        );
        assert!(is_hide(&ops[0], "a"), "{ops:?}");
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// Closing a window drops its views and the observers its item copies held:
// the closed window's card views and its shell are gone, a change to the
// model its copies observed syncs nothing, and the surviving window's own
// sync still reacts to its own model. The other window keeps working — with
// the active window gone, calls act on it as the window that was active
// last. With no window left, a call is refused with an INFO `no window`.
#[gpui::test]
fn closing_a_window_drops_what_it_holds_and_the_other_window_keeps_working(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "tour.names");
            for node in ["a", "b"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.place(cx, "row", "yard.row", "root", "children");
            host.item_slot::<Names>(cx, "row", "children");
            host.place(cx, "name", "tour.card", "row", "children");
        });
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);
        let (first_id, second_id) = (window_id(first, &mut app), window_id(second, &mut app));
        let default: Vec<String> = ["root", "a", "b", "row", "name", "name@ada", "name@bob"]
            .into_iter()
            .map(|id| id.to_string())
            .collect();

        // The first window hides `a`; the second window is the active one.
        activate(first, &mut app);
        app.update(|cx| host.set_hidden(cx, "a", true));
        let first_names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        activate(second, &mut app);
        let second_names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        assert_ne!(
            first_names.entity_id(),
            second_names.entity_id(),
            "the windows share one items model"
        );
        // The shell entity holds the item copies' observers: a weak handle
        // sees whether it, and they, are released.
        let second_shell = second
            .update(&mut app, |_, _, cx| cx.entity().downgrade())
            .unwrap();
        let calls = Rc::new(RefCell::new(0usize));
        let _subscription = {
            let calls = calls.clone();
            app.update(|cx| shell::observe_layout(cx, move |_cx| *calls.borrow_mut() += 1))
        };

        second
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        // The closed window's views are dropped.
        for node in ["a", "b", "name", "name@ada", "name@bob"] {
            assert!(
                view(&views, second_id, node).is_none(),
                "{node}'s view outlived its window"
            );
        }
        assert_eq!(
            app.update(|cx| cx.windows().len()),
            1,
            "the closed window is still in the app"
        );
        assert!(
            second_shell.upgrade().is_none(),
            "the closed window's shell outlived its window, keeping its item observers"
        );
        // The observers the window's copies held are dropped with it: the
        // model they observed notifies nobody, and nothing syncs.
        app.update(|cx| {
            second_names.update(cx, |model, cx| {
                model.0.push("zed".into());
                cx.notify();
            });
        });
        app.run_until_parked();
        assert_eq!(
            inspected(&mut app),
            default,
            "the dead window's items sync ran"
        );
        assert_eq!(
            *calls.borrow(),
            0,
            "the dead window's items sync told the layout observers"
        );
        // The surviving window's own items sync still reacts to its model.
        app.update(|cx| {
            first_names.update(cx, |model, cx| {
                model.0.push("zed".into());
                cx.notify();
            });
        });
        app.run_until_parked();
        assert!(
            inspected(&mut app).contains(&"name@zed".to_string()),
            "the surviving window's items sync did not run"
        );
        assert_eq!(*calls.borrow(), 1);

        // The other window keeps working: with the active window gone, calls
        // act on it as the window that was active last.
        assert!(
            hidden(&mut app, "a"),
            "the call did not reach the surviving window"
        );
        assert!(!hidden(&mut app, "b"));
        app.update(|cx| host.set_hidden(cx, "b", true));
        assert!(hidden(&mut app, "b"));
        window_update(first, &mut app, |window, cx| {
            host.add(window, cx, "still", "tour.card", "root", "children")
        });
        draw(first, &mut app);
        assert!(inspected(&mut app).contains(&"still".to_string()));
        let mut visual = gpui::VisualTestContext::from_window(first.into(), &app);
        assert!(
            visual.debug_bounds("CARD-still").is_some(),
            "the surviving window stopped drawing"
        );

        // With no window left, a call is refused with an INFO `no window`.
        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        for node in ["a", "b", "still"] {
            assert!(
                view(&views, first_id, node).is_none(),
                "{node}'s view outlived its window"
            );
        }
        let mark = trace.mark();
        app.update(|cx| host.set_hidden(cx, "a", false));
        let lines = trace.since(mark);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-op")
                    && line.contains(": INFO")
                    && line.contains("no window")
            }),
            "a call with no window did not log the INFO: {lines:#?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A model an extension gives to the window level after a window is open
// belongs to the application: every window — open now or opened later —
// finds the same model.
#[gpui::test]
fn a_model_given_to_the_window_level_is_found_by_every_window(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            host.place(cx, "a", "tour.card", "root", "children");
        });
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);
        activate(first, &mut app);
        let shared = app.update(|cx| cx.new(|_| Names(vec!["ada".into()])));
        app.update(|cx| host.provide_window(cx, shared.clone()));
        let found = app.update(|cx| shell::context::<Names>(cx, "a"));
        assert_eq!(
            found.as_ref().map(|model| model.entity_id()),
            Some(shared.entity_id()),
            "the window the model was given to cannot find it"
        );
        activate(second, &mut app);
        let found = app.update(|cx| shell::context::<Names>(cx, "a"));
        assert_eq!(
            found.as_ref().map(|model| model.entity_id()),
            Some(shared.entity_id()),
            "the second window found a different window-level model or none"
        );
        let third = open_and_draw(&mut app);
        activate(third, &mut app);
        let found = app.update(|cx| shell::context::<Names>(cx, "a"));
        assert_eq!(
            found.as_ref().map(|model| model.entity_id()),
            Some(shared.entity_id()),
            "a window opened later found a different window-level model or none"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
