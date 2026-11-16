use gpui::{
    AppContext, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    TestAppContext, px,
};
use riso_shell as shell;
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

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs, with the records' groups.
const PROBE: &str = "item_slots_probe";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-item-slots-{}-{}-{nanos:x}",
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
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        RECORDS
            .lock()
            .unwrap()
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

// The model an item slot is over: a list of names.
struct Names(Vec<SharedString>);

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

// A second `Items` model, to check that a second declaration is refused
// whatever its model.
struct OtherNames;

impl shell::Items for OtherNames {
    fn items(&self) -> Vec<SharedString> {
        Vec::new()
    }
}

// A second model the giving component provides, so a copy's `context` can be
// checked for more than one type above the item slot.
struct Count(u32);

type Shown = Rc<RefCell<Vec<(String, Option<String>)>>>;

// The view each node or copy got, so a copy's view can be checked for being
// kept or dropped.
type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

// A card records its node and what `item` returns for it on every frame, and
// holds a count, so state can be given to a copy's view.
struct Card {
    node: String,
    count: u32,
    shown: Shown,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let key = shell::item(cx, &self.node).map(|key| key.to_string());
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
            .child(
                gpui::canvas(
                    move |_, _, _| shown.borrow_mut().push((node.clone(), key.clone())),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

// A view that records its node and draws the nodes of one slot.
struct Container {
    node: String,
    slot: &'static str,
    slots: shell::Slots,
    shown: Shown,
}

impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::canvas(
                    move |_, _, _| shown.borrow_mut().push((node.clone(), None)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(
                self.slots
                    .views(self.slot)
                    .into_iter()
                    .map(|view| gpui::div().flex_1().min_h_0().child(view)),
            )
    }
}

// Refresh before drawing so every view re-renders and records itself in
// `shown` again: `shown` then names every view in the tree, in draw order.
fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
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

// Every node that was drawn, in draw order.
fn drawn(shown: &Shown) -> Vec<String> {
    shown
        .borrow()
        .iter()
        .map(|(node, _)| node.clone())
        .collect()
}

fn shown_pairs(entries: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    entries
        .iter()
        .map(|(node, key)| (node.to_string(), key.map(str::to_string)))
        .collect()
}

// True when `stack` holds each group of `chain` inside the one before it.
fn inside(stack: &[String], chain: &[&str]) -> bool {
    let mut from = 0;
    for group in chain {
        match stack[from..]
            .iter()
            .position(|label| label.starts_with(*group))
        {
            Some(at) => from += at + 1,
            None => return false,
        }
    }
    true
}

// The default tree: `root`, a tour.names that gives the `Names` model (and a
// `Count`), holds `row`, a riso.row node whose `children` slot is declared an
// item slot over `Names`; `name` is the slot's one template.
fn setup(app: &mut TestAppContext) -> (shell::Extension, shell::Extension, Views, Shown) {
    let shown = Shown::default();
    let views = Views::default();
    let (host, guest) = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let guest = shell::Extension::new(cx, "guest").unwrap();
        let container_shown = shown.clone();
        host.register(
            cx,
            "tour.names",
            &["children"],
            move |node, slots, _, cx| {
                cx.new(|_| Container {
                    node: node.to_string(),
                    slot: "children",
                    slots,
                    shown: container_shown.clone(),
                })
                .into()
            },
        );
        host.provide::<Names>(cx, "tour.names", |_, cx| {
            cx.new(|_| Names(vec!["ada".into(), "bob".into(), "cy".into()]))
        });
        host.provide::<Count>(cx, "tour.names", |_, cx| cx.new(|_| Count(10)));
        let card_shown = shown.clone();
        let created = views.clone();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let card = cx.new(|_| Card {
                node: node.to_string(),
                count: 0,
                shown: card_shown.clone(),
            });
            created
                .borrow_mut()
                .insert(node.to_string(), card.downgrade());
            card.into()
        });
        guest.register(cx, "guest.card", &["body"], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        });
        host.place_root(cx, "root", "tour.names");
        host.place(cx, "row", "riso.row", "root", "children");
        host.item_slot::<Names>(cx, "row", "children");
        host.place(cx, "name", "tour.card", "row", "children");
        (host, guest)
    });
    (host, guest, views, shown)
}

// A riso.row whose `children` slot is an item slot shows one copy of its
// template for each item of the model above it, in the items' order, and each
// copy's `item` is its key.
#[gpui::test]
fn a_riso_row_item_slot_shows_one_copy_per_item_in_order_each_with_its_key(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, _guest, _views, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        assert_eq!(
            *shown.borrow(),
            shown_pairs(&[
                ("root", None),
                ("name@ada", Some("ada")),
                ("name@bob", Some("bob")),
                ("name@cy", Some("cy")),
            ])
        );
        // `item` returns the copy's key; the template itself is not a copy.
        assert_eq!(
            app.update(|cx| shell::item(cx, "name@ada")),
            Some(SharedString::from("ada"))
        );
        assert!(app.update(|cx| shell::item(cx, "name")).is_none());
        // Each copy is one column of the row, in the items' order.
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let ada = visual
            .debug_bounds("CARD-name@ada")
            .expect("name@ada is drawn");
        let bob = visual
            .debug_bounds("CARD-name@bob")
            .expect("name@bob is drawn");
        let cy = visual
            .debug_bounds("CARD-name@cy")
            .expect("name@cy is drawn");
        assert!(ada.origin.x < bob.origin.x && bob.origin.x < cy.origin.x);
        // A second template makes its own copy of each item, in the items'
        // order and then the templates' order.
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "other", "tour.card", "row", "children");
            })
            .unwrap();
        draw(window, &mut app, &shown);
        assert_eq!(
            *shown.borrow(),
            shown_pairs(&[
                ("root", None),
                ("name@ada", Some("ada")),
                ("other@ada", Some("ada")),
                ("name@bob", Some("bob")),
                ("other@bob", Some("bob")),
                ("name@cy", Some("cy")),
                ("other@cy", Some("cy")),
            ])
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A new key gets one copy of each shown template; a key that left has its
// copies' views dropped; a key that stayed keeps its copies and their state
// even when the order changes; and no item change records in the user layer.
#[gpui::test]
fn items_coming_and_going_make_and_drop_copies_while_reordering_keeps_them(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx, trace| {
        let temp = TempDir::new();
        let layout_path = temp.path().join("layout.json");
        fs::write(&layout_path, r#"{"version":1,"operations":[]}"#).unwrap();
        let mut app = cx.new_app();
        app.update(|cx| shell::set_layout_file(cx, layout_path.clone()));
        let (_host, _guest, views, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        let names = app.update(|cx| shell::context::<Names>(cx, "name").unwrap());
        let layout_on_disk = fs::read_to_string(&layout_path).unwrap();

        // A new key: one more copy, created inside `item-copy` inside
        // `items-changed`.
        app.update(|cx| {
            names.update(cx, |model, cx| {
                model.0.push("dee".into());
                cx.notify();
            });
        });
        draw(window, &mut app, &shown);
        assert_eq!(
            drawn(&shown),
            ["root", "name@ada", "name@bob", "name@cy", "name@dee"]
        );
        assert_eq!(
            app.update(|cx| shell::item(cx, "name@dee")),
            Some(SharedString::from("dee"))
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(
            fs::read_to_string(&layout_path).unwrap(),
            layout_on_disk,
            "items coming and going do not change the layout file"
        );
        {
            let captured = trace.0.lock().unwrap();
            assert!(
                captured.span_stacks.iter().any(|stack| {
                    inside(stack, &["items-changed", "item-copy", "component-create"])
                        && stack.last().is_some_and(|label| {
                            label.starts_with("component-create") && label.contains("name@dee")
                        })
                        && stack
                            .iter()
                            .any(|label| label.starts_with("item-copy") && label.contains("dee"))
                }),
                "the new key's copy is created inside item-copy inside items-changed: {:#?}",
                captured.span_stacks
            );
        }

        // A key that left: its copy's view is dropped.
        let before = trace.mark();
        app.update(|cx| {
            names.update(cx, |model, cx| {
                model.0.retain(|key| key.as_str() != "ada");
                cx.notify();
            });
        });
        draw(window, &mut app, &shown);
        assert_eq!(drawn(&shown), ["root", "name@bob", "name@cy", "name@dee"]);
        assert!(
            views
                .borrow()
                .get("name@ada")
                .and_then(|view| view.upgrade())
                .is_none(),
            "name@ada's view outlived its item"
        );
        {
            let lines = trace.since(before);
            assert!(
                lines.iter().any(|line| line.starts_with("items-changed")
                    && line.contains("DEBUG")
                    && line.contains("view dropped")
                    && line.contains("name@ada")),
                "the left key's copies are dropped inside items-changed: {lines:#?}"
            );
        }
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(
            fs::read_to_string(&layout_path).unwrap(),
            layout_on_disk,
            "items coming and going do not change the layout file"
        );

        // A reorder keeps the copies' views and their state.
        let bob = views
            .borrow()
            .get("name@bob")
            .and_then(|view| view.upgrade())
            .expect("name@bob has a view");
        let bob_id = bob.entity_id();
        app.update(|cx| bob.update(cx, |card, _| card.count = 42));
        let before = trace.mark();
        app.update(|cx| {
            names.update(cx, |model, cx| {
                model.0 = vec!["cy".into(), "dee".into(), "bob".into()];
                cx.notify();
            });
        });
        draw(window, &mut app, &shown);
        assert_eq!(drawn(&shown), ["root", "name@cy", "name@dee", "name@bob"]);
        let bob_after = views
            .borrow()
            .get("name@bob")
            .and_then(|view| view.upgrade())
            .expect("name@bob has a view");
        assert_eq!(
            bob_after.entity_id(),
            bob_id,
            "name@bob's view was recreated"
        );
        assert_eq!(app.update(|cx| bob_after.read(cx).count), 42);
        {
            let lines = trace.since(before);
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains("view dropped") && line.contains("name@")),
                "no copy is dropped on a reorder: {lines:#?}"
            );
        }
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(
            fs::read_to_string(&layout_path).unwrap(),
            layout_on_disk,
            "items coming and going do not change the layout file"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// Hiding a template hides every copy of it; removing it drops the copies'
// views. Both are ordinary operations on the template: they are recorded.
#[gpui::test]
fn hiding_a_template_hides_every_copy_and_removing_it_removes_them(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, _guest, views, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        app.update(|cx| host.set_hidden(cx, "name", true));
        draw(window, &mut app, &shown);
        assert_eq!(drawn(&shown), ["root"]);
        assert!(app.update(|cx| shell::is_hidden(cx, "name")));
        app.update(|cx| host.set_hidden(cx, "name", false));
        draw(window, &mut app, &shown);
        assert_eq!(
            *shown.borrow(),
            shown_pairs(&[
                ("root", None),
                ("name@ada", Some("ada")),
                ("name@bob", Some("bob")),
                ("name@cy", Some("cy")),
            ])
        );
        let before = trace.mark();
        app.update(|cx| host.remove(cx, "name"));
        draw(window, &mut app, &shown);
        assert_eq!(drawn(&shown), ["root"]);
        for copy in ["name@ada", "name@bob", "name@cy"] {
            assert!(
                views
                    .borrow()
                    .get(copy)
                    .and_then(|view| view.upgrade())
                    .is_none(),
                "{copy}'s view outlived its template"
            );
        }
        let lines = trace.since(before);
        for copy in ["name@ada", "name@bob", "name@cy"] {
            assert!(
                lines
                    .iter()
                    .any(|line| line.contains("view dropped") && line.contains(copy)),
                "missing `view dropped` for {copy} in {lines:#?}"
            );
        }
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 3);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A copy has the ID `<template>@<key>` and is not a node of the tree: an
// operation that names it is refused with an INFO. An ID with `@` that is
// not `<slug>@<slug>` is an invalid ID, not a copy.
#[gpui::test]
fn an_operation_that_names_a_copy_is_refused_with_the_info(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, guest, _views, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        let before = trace.mark();
        app.update(|cx| {
            host.set_hidden(cx, "name@ada", true);
            guest.remove(cx, "name@ada");
            host.move_node(cx, "name@ada", "root", "children", 0);
            host.set_hidden(cx, "name@Ada", true);
            host.remove(cx, "name@ada@x");
        });
        draw(window, &mut app, &shown);
        // Every copy is still there, and nothing was recorded.
        assert_eq!(drawn(&shown), ["root", "name@ada", "name@bob", "name@cy"]);
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        let lines: Vec<String> = trace
            .since(before)
            .iter()
            .map(|line| line.replace('"', ""))
            .collect();
        for (op, extension) in [("hide", "tour"), ("remove", "guest"), ("move", "tour")] {
            let group = format!("layout-op op={op} node=name@ada extension={extension}: INFO");
            assert!(
                lines.iter().any(|line| line.starts_with(&group)
                    && line.contains("name@ada is a copy of an item slot")),
                "missing the refused {op} in {lines:#?}"
            );
        }
        // `name@Ada` and `name@ada@x` are not `<slug>@<slug>`: each gets an
        // invalid-ID ERROR, and neither is called a copy.
        for bad in ["name@Ada", "name@ada@x"] {
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains(&format!("{bad} is a copy"))),
                "{bad} is not a copy: {lines:#?}"
            );
        }
        let errors = errors();
        assert_eq!(errors.len(), 2, "{errors:?}");
        for bad in ["name@Ada", "name@ada@x"] {
            assert!(
                errors
                    .iter()
                    .any(|message| message.contains("invalid") && message.contains(bad)),
                "missing the invalid-ID ERROR for {bad} in {errors:?}"
            );
        }
    });
}

// An item slot whose model is found nowhere shows nothing and warns once.
#[gpui::test]
fn an_item_slot_with_no_model_shows_nothing_and_warns_once(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let shown = Shown::default();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let card_shown = shown.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    count: 0,
                    shown: card_shown.clone(),
                })
                .into()
            });
            host.place_root(cx, "root", "riso.row");
            host.item_slot::<Names>(cx, "root", "children");
            host.place(cx, "name", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app, &shown);
        assert!(shown.borrow().is_empty(), "{:?}", *shown.borrow());
        assert!(app.update(|cx| shell::item(cx, "name@ada")).is_none());
        // A second frame still warns nothing more: the WARN is logged once.
        draw(window, &mut app, &shown);
        let warnings = warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("model"),
            "the one WARN is the missing-model one: {warnings:?}"
        );
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// `context` from a copy finds the models above its item slot, as from the
// template.
#[gpui::test]
fn a_copys_context_finds_the_models_above_its_item_slot(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (_host, _guest, _views, shown) = setup(&mut app);
        let _window = open_and_draw(&mut app, &shown);
        let (copy_names, template_names, row_names, copy_count) = app.update(|cx| {
            (
                shell::context::<Names>(cx, "name@ada").unwrap(),
                shell::context::<Names>(cx, "name").unwrap(),
                shell::context::<Names>(cx, "row").unwrap(),
                shell::context::<Count>(cx, "name@ada").unwrap(),
            )
        });
        // The same model the item slot reads: `root`'s.
        assert_eq!(copy_names.entity_id(), template_names.entity_id());
        assert_eq!(copy_names.entity_id(), row_names.entity_id());
        // Any model type above the item slot, for every copy.
        assert_eq!(app.update(|cx| copy_count.read(cx).0), 10);
        let bob_names = app.update(|cx| shell::context::<Names>(cx, "name@bob").unwrap());
        assert_eq!(bob_names.entity_id(), copy_names.entity_id());
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// `item_slot` is refused with an ERROR for an invalid node ID or slot name,
// a node that is not in the tree or that another extension placed, a slot the
// node's component does not declare (checked when the component registers if
// it has not yet), or a second declaration for the same node and slot.
#[gpui::test]
fn the_declaration_refusals_each_log_an_error(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, guest, _views, shown) = setup(&mut app);
        app.update(|cx| {
            host.item_slot::<Names>(cx, "bad node", "children");
            host.item_slot::<Names>(cx, "row", "bad slot");
            host.item_slot::<Names>(cx, "missing", "children");
            // `theirs` is in the tree but was placed by `guest`.
            guest.place(cx, "theirs", "guest.card", "root", "children");
            host.item_slot::<Names>(cx, "theirs", "body");
            // `riso.row` is registered and declares no slot `nope`.
            host.item_slot::<Names>(cx, "row", "nope");
            // `row.children` is already an item slot over `Names`, from the
            // setup: a second declaration, with any model, is refused.
            host.item_slot::<Names>(cx, "row", "children");
            host.item_slot::<OtherNames>(cx, "row", "children");
            // `frame`'s `nope` is declared before `tour.frame` registers and
            // dropped at it; its `content` stands and works.
            host.place(cx, "frame", "tour.frame", "root", "children");
            host.item_slot::<Names>(cx, "frame", "nope");
            host.item_slot::<Names>(cx, "frame", "content");
            let frame_shown = shown.clone();
            host.register(cx, "tour.frame", &["content"], move |node, slots, _, cx| {
                cx.new(|_| Container {
                    node: node.to_string(),
                    slot: "content",
                    slots,
                    shown: frame_shown.clone(),
                })
                .into()
            });
            host.item_slot::<Names>(cx, "frame", "pocket");
            host.item_slot::<Names>(cx, "frame", "content");
            host.place(cx, "other", "tour.card", "frame", "content");
        });
        let _window = open_and_draw(&mut app, &shown);
        // The declarations that stand still work: `frame.content` is an item
        // slot, and `other` is its template.
        assert_eq!(
            *shown.borrow(),
            shown_pairs(&[
                ("root", None),
                ("name@ada", Some("ada")),
                ("name@bob", Some("bob")),
                ("name@cy", Some("cy")),
                ("frame", None),
                ("other@ada", Some("ada")),
                ("other@bob", Some("bob")),
                ("other@cy", Some("cy")),
            ])
        );
        let errors = errors();
        assert_eq!(errors.len(), 10, "{errors:?}");
        // Each refusal is an ERROR inside its own `item-slot-declare` group.
        let lines: Vec<String> = trace
            .0
            .lock()
            .unwrap()
            .lines
            .iter()
            .map(|line| line.replace('"', ""))
            .collect();
        for (node, slot, count) in [
            ("bad node", "children", 1),
            ("row", "bad slot", 1),
            ("missing", "children", 1),
            ("theirs", "body", 1),
            ("row", "nope", 1),
            ("row", "children", 2),
            ("frame", "nope", 1),
            ("frame", "pocket", 1),
            ("frame", "content", 1),
        ] {
            let group = format!("item-slot-declare node={node} slot={slot}");
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.starts_with(&group) && line.contains(": ERROR"))
                    .count(),
                count,
                "the refused {node}.{slot} inside its group: {lines:#?}"
            );
        }
    });
}

// The declaration names one node, as Zed's pane is one instance: `row`'s
// `children` slot repeats its templates, and another `riso.row` node shows
// its nodes once, as before.
#[gpui::test]
fn a_declaration_on_one_riso_row_leaves_another_riso_row_unchanged(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, _guest, _views, shown) = setup(&mut app);
        app.update(|cx| {
            host.place(cx, "other_row", "riso.row", "root", "children");
            host.place(cx, "plain", "tour.card", "other_row", "children");
        });
        let _window = open_and_draw(&mut app, &shown);
        assert_eq!(
            *shown.borrow(),
            shown_pairs(&[
                ("root", None),
                ("name@ada", Some("ada")),
                ("name@bob", Some("bob")),
                ("name@cy", Some("cy")),
                ("plain", None),
            ])
        );
        assert!(app.update(|cx| shell::item(cx, "plain@ada")).is_none());
        assert!(app.update(|cx| shell::item(cx, "plain")).is_none());
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// `riso::ToggleZoom` and `riso::Hide` on a focused copy are refused with the
// INFO, and so is `Extension::set_zoomed` naming one: a copy is not a node
// of the tree, and the layout's operations do not apply to it.
#[gpui::test]
fn zooming_or_hiding_a_focused_copy_is_refused_with_the_info(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, _guest, _views, shown) = setup(&mut app);
        let window = open_and_draw(&mut app, &shown);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let card = visual
            .debug_bounds("CARD-name@ada")
            .expect("name@ada's card draws");
        visual.simulate_click(card.center(), gpui::Modifiers::default());
        draw(window, &mut app, &shown);
        assert_eq!(
            visual
                .update(|window, cx| {
                    shell::focused_node(window, cx).map(|node| node.to_string())
                })
                .as_deref(),
            Some("name@ada")
        );
        let lines_since = |mark: usize| -> Vec<String> {
            trace
                .since(mark)
                .iter()
                .map(|line| line.replace('"', ""))
                .collect()
        };
        let before = trace.mark();
        visual.dispatch_action(shell::ToggleZoom);
        assert!(
            lines_since(before).iter().any(|line| {
                line.contains(": INFO") && line.contains("name@ada is a copy of an item slot")
            }),
            "missing the refused ToggleZoom's INFO: {:#?}",
            lines_since(before)
        );
        // The Hide action runs the `hide` operation, so its INFO is inside
        // `layout-op`.
        let before = trace.mark();
        visual.dispatch_action(shell::Hide);
        assert!(
            lines_since(before).iter().any(|line| {
                line.starts_with("layout-op op=hide node=name@ada extension=riso: INFO")
                    && line.contains("name@ada is a copy of an item slot")
            }),
            "missing the refused Hide's INFO: {:#?}",
            lines_since(before)
        );
        let before = trace.mark();
        app.update(|cx| host.set_zoomed(cx, "name@ada"));
        assert!(
            lines_since(before).iter().any(|line| {
                line.contains(": INFO") && line.contains("name@ada is a copy of an item slot")
            }),
            "missing the refused set_zoomed's INFO: {:#?}",
            lines_since(before)
        );
        draw(window, &mut app, &shown);
        // Nothing changed: no zoom, no hide, nothing recorded, and the copy
        // still has the focus.
        assert!(app.update(|cx| shell::zoomed(cx)).is_none());
        assert!(!app.update(|cx| shell::is_hidden(cx, "name")));
        assert_eq!(drawn(&shown), ["root", "name@ada", "name@bob", "name@cy"]);
        assert_eq!(
            visual
                .update(|window, cx| {
                    shell::focused_node(window, cx).map(|node| node.to_string())
                })
                .as_deref(),
            Some("name@ada")
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}

// A copy's tab in a `riso.tabs` item slot has no close button, as a required
// node's does not, and a middle click on it changes nothing.
#[gpui::test]
fn a_copys_tab_has_no_close_button(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let (host, _guest, views, shown) = setup(&mut app);
        app.update(|cx| {
            host.place(cx, "tabs", "riso.tabs", "root", "children");
            host.item_slot::<Names>(cx, "tabs", "children");
            host.place(cx, "tab_card", "tour.card", "tabs", "children");
            host.place(cx, "plain_tabs", "riso.tabs", "root", "children");
            host.place(cx, "pin", "tour.card", "plain_tabs", "children");
        });
        let window = open_and_draw(&mut app, &shown);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        assert_eq!(
            app.update(|cx| { shell::selected_tab(cx, "tabs").map(|node| node.to_string()) })
                .as_deref(),
            Some("tab_card@ada")
        );
        assert!(visual.debug_bounds("TAB-tab_card@ada").is_some());
        assert!(
            visual.debug_bounds("CLOSE-pin").is_some(),
            "a plain tab's close button draws"
        );
        assert!(
            visual.debug_bounds("CLOSE-tab_card@ada").is_none(),
            "a copy's tab has no close button"
        );
        // A middle click on a copy's tab changes nothing.
        let tab = visual
            .debug_bounds("TAB-tab_card@ada")
            .expect("the copy's tab draws")
            .center();
        visual.simulate_mouse_down(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
        visual.simulate_mouse_up(tab, gpui::MouseButton::Middle, gpui::Modifiers::default());
        draw(window, &mut app, &shown);
        assert!(
            visual.debug_bounds("TAB-tab_card@ada").is_some(),
            "the copy's tab stays"
        );
        assert!(
            views
                .borrow()
                .get("tab_card@ada")
                .and_then(|view| view.upgrade())
                .is_some(),
            "the copy's view is not dropped"
        );
        assert_eq!(
            app.update(|cx| { shell::selected_tab(cx, "tabs").map(|node| node.to_string()) })
                .as_deref(),
            Some("tab_card@ada"),
            "the selection does not change"
        );
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
        assert!(errors().is_empty(), "{:?}", errors());
    });
}
