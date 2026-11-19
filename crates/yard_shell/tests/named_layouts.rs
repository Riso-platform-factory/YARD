use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use serde_json::json;
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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-named-layouts-{}-{}-{nanos:x}",
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

fn warnings() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, message)| message)
        .collect()
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
        if attributes.metadata().target().starts_with("yard_shell") {
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
        if !(target.starts_with("yard_shell") || target.starts_with("yard_layout"))
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
    fn stacks(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().span_stacks.clone()
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

// A card fills the zone its container gives it and is found as `CARD-<node>`;
// its `count` is view state a test can set and read back.
struct Card {
    node: String,
    count: u32,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

// The view each node got, so a card's identity and state can be checked for
// being kept or dropped.
type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

fn extension(app: &mut TestAppContext) -> (shell::Extension, Views) {
    let views = Views::default();
    let host = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                count: 0,
            });
            created
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
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

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn focused(visual: &mut gpui::VisualTestContext) -> Option<String> {
    visual.update(|window, cx| shell::focused_node(window, cx).map(|node| node.to_string()))
}

fn view(views: &Views, node: &str) -> gpui::Entity<Card> {
    views.borrow()[node]
        .upgrade()
        .unwrap_or_else(|| panic!("{node} has a view"))
}

fn view_id(views: &Views, node: &str) -> gpui::EntityId {
    view(views, node).entity_id()
}

fn show_layout(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    host: &shell::Extension,
    name: &str,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| host.show_layout(window, cx, name))
        .unwrap();
}

fn reset_layout(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    host: &shell::Extension,
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| host.reset_layout(window, cx))
        .unwrap();
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

// `debug_bounds` takes a `&'static str`: a selector made at runtime is leaked.
fn selector(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

// The named cards are drawn in this order along their row or column.
fn assert_order(visual: &mut gpui::VisualTestContext, order: &[&str]) {
    let mut previous: Option<gpui::Pixels> = None;
    for node in order {
        let bounds = visual
            .debug_bounds(selector(format!("CARD-{node}")))
            .unwrap_or_else(|| panic!("CARD-{node} is not drawn"));
        if let Some(x) = previous {
            assert!(x < bounds.origin.x, "CARD-{node} is out of order");
        }
        previous = Some(bounds.origin.x);
    }
}

// True when `line` holds `node` as a `node` field, quoted or not.
fn node_field(line: &str, node: &str) -> bool {
    line.contains(&format!("node=\"{node}\""))
        || line.contains(&format!("node={node} "))
        || line.ends_with(&format!("node={node}"))
}

// A window starts on `yard.default`; a named layout's operations apply over
// the default when the window switches to it, and switching back shows the
// default with its own user layer.
#[gpui::test]
fn a_window_starts_with_yard_default_and_switching_back_gives_the_default_with_its_layer(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("data").join("layout.json");
        let layouts = temp.path().join("data").join("layouts");
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
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert_eq!(shown(window, &mut app), "yard.default");
        assert_order(&mut visual, &["a", "b", "c"]);

        // One operation in the default's user layer: `b` hidden.
        app.update(|cx| host.set_hidden(cx, "b", true));
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-b").is_none());
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);

        // The swap is the default plus `move c first`; the default's layer is
        // not the swap's, so `b` shows again.
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        assert_eq!(shown(window, &mut app), "tour.swap");
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_order(&mut visual, &["c", "a", "b"]);

        // Back on `yard.default`: the order is the default's and `b` is hidden
        // again by the default's own user layer.
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        assert_eq!(shown(window, &mut app), "yard.default");
        assert!(visual.debug_bounds("CARD-b").is_none());
        assert_order(&mut visual, &["a", "c"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A node shown before and after a switch keeps its view and its state; a node
// that leaves has its view dropped; a node that comes has its view created.
// The focus stays on its node while it is shown and goes to the window when
// the node leaves.
#[gpui::test]
fn a_switch_keeps_views_of_shared_nodes_drops_leavers_and_creates_comers(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.named_layout(cx, "tour.diet", |layout| {
                layout.remove("a");
                layout.add("d", "tour.card", "root", "children");
            });
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let a_id = view_id(&views, "a");
        let b_id = view_id(&views, "b");
        app.update(|cx| view(&views, "b").update(cx, |card, _| card.count = 7));
        click(&mut visual, "CARD-b");
        draw(window, &mut app);
        assert_eq!(focused(&mut visual).as_deref(), Some("b"));

        show_layout(window, &mut app, &host, "tour.diet");
        draw(window, &mut app);
        // `a` left: its view is dropped. `b` and `c` stayed: same entities,
        // same state. `d` came: a fresh view.
        assert!(
            views.borrow()["a"].upgrade().is_none(),
            "a's view outlived it"
        );
        assert_eq!(view_id(&views, "b"), b_id, "b's view was recreated");
        assert_eq!(app.update(|cx| view(&views, "b").read(cx).count), 7);
        assert!(views.borrow()["c"].upgrade().is_some());
        assert!(views.borrow()["d"].upgrade().is_some());
        assert_eq!(app.update(|cx| view(&views, "d").read(cx).count), 0);
        assert!(visual.debug_bounds("CARD-a").is_none());
        assert_order(&mut visual, &["b", "c", "d"]);
        assert_eq!(
            focused(&mut visual).as_deref(),
            Some("b"),
            "b is still shown, so it keeps the focus"
        );

        // Focus a node the next switch removes: the focus goes to the window.
        click(&mut visual, "CARD-d");
        draw(window, &mut app);
        assert_eq!(focused(&mut visual).as_deref(), Some("d"));
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        assert!(
            focused(&mut visual).is_none(),
            "d left, so the window took the focus"
        );
        assert!(
            views.borrow()["d"].upgrade().is_none(),
            "d's view outlived it"
        );
        // `a` comes back with a new view and fresh state.
        assert_ne!(
            view_id(&views, "a"),
            a_id,
            "a's view was kept over its absence"
        );
        assert_eq!(app.update(|cx| view(&views, "a").read(cx).count), 0);
        assert_order(&mut visual, &["a", "b", "c"]);
        assert_eq!(shown(window, &mut app), "yard.default");
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// An operation made while a named layout is shown is recorded in that
// layout's user layer and saved to its file only. Switching away and back
// restores it, a reset drops only the shown layout's layer, and a restart
// opens on `yard.default`.
#[gpui::test]
fn an_operation_while_a_named_layout_is_shown_is_saved_to_its_file_only_and_a_reset_drops_only_it(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("data").join("layout.json");
        let layouts = temp.path().join("data").join("layouts");
        let swap_path = layouts.join("tour.swap.json");
        let place = |app: &mut TestAppContext, host: &shell::Extension| {
            app.update(|cx| {
                host.place_root(cx, "root", "yard.row");
                for node in ["a", "b", "c"] {
                    host.place(cx, node, "tour.card", "root", "children");
                }
                host.named_layout(cx, "tour.swap", |layout| {
                    layout.move_node("c", "root", "children", 0);
                });
            });
        };
        let mut app = cx.new_app();
        let files = layouts.clone();
        app.update(|cx| {
            shell::set_layout_file(cx, layout_path.clone());
            shell::set_named_layout_files(cx, move |name| files.join(format!("{name}.json")));
        });
        let (host, _views) = extension(&mut app);
        place(&mut app, &host);
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        // `b` hidden is the default layout's user layer, saved to layout.json.
        app.update(|cx| host.set_hidden(cx, "b", true));
        let ops = saved_ops(&layout_path);
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(is_hide(&ops[0], "b"), "{ops:?}");

        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        // `c` hidden is the swap's user layer, saved to its file only.
        app.update(|cx| host.set_hidden(cx, "c", true));
        let ops = saved_ops(&swap_path);
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(is_hide(&ops[0], "c"), "{ops:?}");
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.len(),
            1,
            "the default's file got the swap's operation: {ops:?}"
        );
        assert!(is_hide(&ops[0], "b"), "{ops:?}");

        // Away and back: the file is read again and `c` is hidden in the swap.
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-c").is_some());
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("CARD-c").is_none(),
            "the swap's saved hide is applied again"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        assert_order(&mut visual, &["a", "b"]);

        // A reset drops the shown layout's layer only: `c` is back before `a`,
        // and the default's file still holds its own hide.
        reset_layout(window, &mut app, &host);
        draw(window, &mut app);
        assert_eq!(shown(window, &mut app), "tour.swap");
        assert_order(&mut visual, &["c", "a", "b"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        let ops = saved_ops(&layout_path);
        assert_eq!(
            ops.len(),
            1,
            "the default's file was rewritten by the swap's reset: {ops:?}"
        );
        assert!(is_hide(&ops[0], "b"), "{ops:?}");

        // The drop reaches the file: away and back, `c` is shown in the swap.
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("CARD-c").is_some(),
            "the reset's layer came back from the file"
        );

        // A restart opens on `yard.default`, restored from layout.json.
        {
            let mut app = cx.new_app();
            let files = layouts.clone();
            app.update(|cx| {
                shell::set_layout_file(cx, layout_path.clone());
                shell::set_named_layout_files(cx, move |name| files.join(format!("{name}.json")));
            });
            let (host, _views) = extension(&mut app);
            place(&mut app, &host);
            let window = open_and_draw(&mut app);
            let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
            assert_eq!(shown(window, &mut app), "yard.default");
            assert!(visual.debug_bounds("CARD-b").is_none());
            assert!(visual.debug_bounds("CARD-c").is_some());
            assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        }
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A named layout's operation that the rules refuse is skipped with an INFO,
// and the rest of its operations apply.
#[gpui::test]
fn a_named_layouts_refused_operations_are_skipped_with_an_info_and_the_rest_applies(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.named_layout(cx, "tour.mixed", |layout| {
                // `a` is in the default tree: a second add is refused.
                layout.add("a", "tour.card", "root", "children");
                // `ghost` is nowhere: the remove is refused.
                layout.remove("ghost");
                layout.set_hidden("b", true);
                layout.set_props("a", json!({"note": "kept"}));
                layout.add_with_props("d", "tour.card", "root", "children", json!({"note": "new"}));
            });
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let before = trace.mark();
        show_layout(window, &mut app, &host, "tour.mixed");
        draw(window, &mut app);
        let lines = trace.since(before);
        for node in ["a", "ghost"] {
            assert!(
                lines.iter().any(|line| {
                    line.contains(": INFO") && line.contains("skipped") && node_field(line, node)
                }),
                "missing the skipped INFO for {node} in {lines:#?}"
            );
        }
        // The rest applied: `b` is hidden and `d` is added at the end.
        assert!(visual.debug_bounds("CARD-b").is_none());
        assert_order(&mut visual, &["a", "c", "d"]);
        assert_eq!(shown(window, &mut app), "tour.mixed");
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// The registration refusals each log an ERROR, an unknown name keeps the
// shown layout with an INFO, and showing the layout already shown changes
// nothing beyond a `debug` `already shown`.
#[gpui::test]
fn registration_refusals_unknown_names_and_reshowing_the_shown_layout(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let mut app = cx.new_app();
        let files = temp.path().join("data").join("layouts");
        app.update(|cx| {
            shell::set_layout_file(cx, temp.path().join("data").join("layout.json"));
            shell::set_named_layout_files(cx, move |name| files.join(format!("{name}.json")));
        });
        let (host, _views) = extension(&mut app);
        app.update(|cx| {
            host.place_root(cx, "root", "yard.row");
            for node in ["a", "b", "c"] {
                host.place(cx, node, "tour.card", "root", "children");
            }
            host.named_layout(cx, "tour.dup", |layout| {
                layout.set_hidden("b", true);
            });
            // Each refusal logs one ERROR: an invalid name, a name outside the
            // extension's namespace, `yard.default`, and a second registration.
            for name in ["bad name", "other.swap", "yard.default", "tour.dup"] {
                let before = errors().len();
                host.named_layout(cx, name, |_| {});
                assert!(
                    errors().len() > before,
                    "the registration of {name:?} logged no ERROR"
                );
            }
        });
        assert_eq!(errors().len(), 4, "{:?}", errors());
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        // A refused registration registered nothing: `other.swap` is unknown.
        let before = trace.mark();
        show_layout(window, &mut app, &host, "other.swap");
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-switch")
                    && line.contains(": INFO")
                    && line.contains("no named layout")
                    && line.contains("other.swap")
            }),
            "missing the unknown-name INFO in {lines:#?}"
        );
        assert_eq!(shown(window, &mut app), "yard.default");

        // The window already shows `yard.default`: showing it again logs
        // `already shown` and changes nothing — no restore and no switch.
        let before = trace.mark();
        let stacks_before = trace.stacks().len();
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-switch")
                    && line.contains(": DEBUG")
                    && line.contains("already shown")
            }),
            "missing `already shown` in {lines:#?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("switched"))
                && !trace.stacks()[stacks_before..]
                    .iter()
                    .flatten()
                    .any(|label| label.starts_with("layout-restore")),
            "reshowing the shown layout did work: {lines:#?}"
        );
        assert_eq!(shown(window, &mut app), "yard.default");
        assert_order(&mut visual, &["a", "b", "c"]);

        // Switch to `tour.dup`, which hides `b`; showing it again is
        // `already shown`, and an unknown name keeps it.
        show_layout(window, &mut app, &host, "tour.dup");
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-b").is_none());
        assert_eq!(shown(window, &mut app), "tour.dup");
        let before = trace.mark();
        show_layout(window, &mut app, &host, "tour.dup");
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-switch") && line.contains("already shown")
            }),
            "missing `already shown` in {lines:#?}"
        );
        let before = trace.mark();
        show_layout(window, &mut app, &host, "tour.ghost");
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-switch")
                    && line.contains(": INFO")
                    && line.contains("no named layout")
                    && line.contains("tour.ghost")
            }),
            "missing the unknown-name INFO in {lines:#?}"
        );
        draw(window, &mut app);
        assert_eq!(shown(window, &mut app), "tour.dup");
        assert!(visual.debug_bounds("CARD-b").is_none());
        assert_eq!(errors().len(), 4, "{:?}", errors());
    });
}

// A switch ends zoom, as a reset ends it.
#[gpui::test]
fn a_switch_ends_zoom(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
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
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        app.update(|cx| host.set_zoomed(cx, "b"));
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("b"));
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-b").is_some());
        assert!(visual.debug_bounds("CARD-a").is_none());

        show_layout(window, &mut app, &host, "tour.swap");
        assert!(
            app.update(|cx| shell::zoomed(cx)).is_none(),
            "the switch kept the zoom"
        );
        draw(window, &mut app);
        assert_order(&mut visual, &["c", "a", "b"]);
        assert_eq!(shown(window, &mut app), "tour.swap");
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// Without `set_named_layout_files` the named layouts' user layers live in
// memory only, warned once: the layer survives a switch away and back.
#[gpui::test]
fn without_named_layout_files_the_layers_live_in_memory_only_with_a_warn_once(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
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
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        app.update(|cx| host.set_hidden(cx, "c", true));
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-c").is_none());
        show_layout(window, &mut app, &host, "yard.default");
        draw(window, &mut app);
        assert!(visual.debug_bounds("CARD-c").is_some());
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("CARD-c").is_none(),
            "the in-memory layer kept the hide across the switches"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 1);
        assert_eq!(warnings().len(), 1, "{:?}", warnings());
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A named layout's file that cannot be read warns once, as at restore, stays
// untouched, and the layout's own operations still apply.
#[gpui::test]
fn a_broken_named_layout_file_warns_and_applies_no_user_layer(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("data").join("layout.json");
        let layouts = temp.path().join("data").join("layouts");
        let swap_path = layouts.join("tour.swap.json");
        fs::create_dir_all(&layouts).unwrap();
        fs::write(&swap_path, "not json").unwrap();
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
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let warns_before = warnings().len();
        show_layout(window, &mut app, &host, "tour.swap");
        draw(window, &mut app);
        // The broken file warns, is left untouched, and the swap still applies.
        let warns = warnings();
        assert_eq!(warns.len(), warns_before + 1, "{warns:?}");
        assert!(
            warns.last().unwrap().contains("cannot be read"),
            "the WARN is the unreadable saved layout: {warns:?}"
        );
        assert_eq!(fs::read_to_string(&swap_path).unwrap(), "not json");
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_order(&mut visual, &["c", "a", "b"]);

        // The next operation rewrites the file.
        app.update(|cx| host.set_hidden(cx, "c", true));
        let ops = saved_ops(&swap_path);
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(is_hide(&ops[0], "c"), "{ops:?}");
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
