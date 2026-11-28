use gpui::{
    AppContext, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    TestAppContext, px,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, Once},
};
use yard_shell as shell;

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
        if record.target().starts_with("yard_shell")
            && matches!(record.level(), log::Level::Warn | log::Level::Error)
        {
            LOGS.lock().unwrap().push(record.args().to_string());
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

struct Names(Vec<SharedString>);
impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

struct Card {
    node: String,
    count: usize,
    titles: Rc<RefCell<Vec<String>>>,
}
impl shell::Titled for Card {
    fn title(&self, cx: &gpui::App) -> SharedString {
        self.titles.borrow_mut().push(self.node.clone());
        shell::item(cx, &self.node)
            .map(|key| format!("Title {key}").into())
            .unwrap_or_else(|| "Title".into())
    }
}
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .size_full()
            .debug_selector(|| format!("CARD-{}", self.node))
    }
}
struct Frame(shell::Slots);
impl gpui::Render for Frame {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().flex().children(
            self.0
                .views("children")
                .into_iter()
                .map(|view| gpui::div().flex_1().min_w_0().child(view)),
        )
    }
}

struct Fixture {
    host: shell::Extension,
    views: Views,
    titles: Rc<RefCell<Vec<String>>>,
    model: gpui::Entity<Names>,
    window: gpui::WindowHandle<shell::Shell>,
}

fn setup(app: &mut TestAppContext, tabs: bool, hidden: bool, two: bool) -> Fixture {
    let views = Views::default();
    let titles = Rc::new(RefCell::new(Vec::new()));
    let (host, model) = app.update(|cx| {
        yard_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let model = cx.new(|_| Names(vec!["ada".into(), "bob".into()]));
        host.provide_window(cx, model.clone());
        host.register(cx, "tour.frame", &["children"], |_, slots, _, cx| {
            cx.new(|_| Frame(slots)).into()
        });
        let made = views.clone();
        let read_titles = titles.clone();
        host.register(cx, "tour.card", &[], move |node, _, _, cx| {
            let view = cx.new(|_| Card {
                node: node.to_string(),
                count: 0,
                titles: read_titles.clone(),
            });
            made.borrow_mut().insert(node.to_string(), view.downgrade());
            view.into()
        });
        host.register_title::<Card>(cx, "tour.card");
        host.place_root(cx, "root", "tour.frame");
        let component = if tabs { "yard.tabs" } else { "yard.row" };
        host.place(cx, "left", component, "root", "children");
        host.place(cx, "right", component, "root", "children");
        host.item_slot::<Names>(cx, "left", "children");
        host.place(cx, "name", "tour.card", "left", "children");
        if two {
            host.place(cx, "other", "tour.card", "left", "children");
        }
        if hidden {
            host.set_hidden(cx, "name", true);
        }
        (host, model)
    });
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(1000.), px(600.)));
    draw(window, app);
    Fixture {
        host,
        views,
        titles,
        model,
        window,
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}
fn card(f: &Fixture, name: &str) -> gpui::Entity<Card> {
    f.views
        .borrow()
        .get(name)
        .and_then(gpui::WeakEntity::upgrade)
        .unwrap_or_else(|| panic!("missing {name}"))
}

#[gpui::test]
fn tabs_show_one_tab_per_item_and_use_component_titles(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, true, false, false);
        let mut visual = gpui::VisualTestContext::from_window(f.window.into(), cx);
        assert!(visual.debug_bounds("TAB-name@ada").is_some());
        assert!(visual.debug_bounds("TAB-name@bob").is_some());
        assert_eq!(
            cx.update(|cx| shell::selected_tab(cx, "left").unwrap().to_string()),
            "name@ada"
        );
        assert!(f.titles.borrow().iter().any(|node| node == "name@ada"));
        assert!(f.titles.borrow().iter().any(|node| node == "name@bob"));
        let ada = card(&f, "name@ada");
        assert_eq!(
            cx.update(|cx| shell::Titled::title(ada.read(cx), cx)),
            "Title ada"
        );
    });
}

#[gpui::test]
fn hidden_templates_create_nothing_until_shown_and_keep_copy_state(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, true, false);
        assert!(f.views.borrow().is_empty());
        cx.update(|cx| f.host.set_hidden(cx, "name", false));
        draw(f.window, cx);
        let ada = card(&f, "name@ada");
        let id = ada.entity_id();
        cx.update(|cx| ada.update(cx, |card, _| card.count = 9));
        cx.update(|cx| f.host.set_hidden(cx, "name", true));
        draw(f.window, cx);
        let mut visual = gpui::VisualTestContext::from_window(f.window.into(), cx);
        assert!(visual.debug_bounds("CARD-name@ada").is_none());
        cx.update(|cx| f.host.set_hidden(cx, "name", false));
        draw(f.window, cx);
        assert_eq!(card(&f, "name@ada").entity_id(), id);
        assert_eq!(cx.update(|cx| ada.read(cx).count), 9);
    });
}

#[gpui::test]
fn moving_a_template_changes_its_slot_and_preserves_copy_identity(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let ada = card(&f, "name@ada");
        cx.update(|cx| f.host.item_slot::<Names>(cx, "right", "children"));
        let before = cx.update(|cx| shell::node_bounds(cx, "name@ada").unwrap());
        cx.update(|cx| f.host.move_node(cx, "name", "right", "children", 0));
        draw(f.window, cx);
        assert_eq!(card(&f, "name@ada").entity_id(), ada.entity_id());
        let after = cx.update(|cx| shell::node_bounds(cx, "name@ada").unwrap());
        assert!(after.origin.x > before.origin.x);
        cx.update(|cx| f.host.move_node(cx, "name", "root", "children", 0));
        draw(f.window, cx);
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_none());
        assert!(
            f.views
                .borrow()
                .get("name")
                .and_then(gpui::WeakEntity::upgrade)
                .is_some()
        );
    });
}

#[gpui::test]
fn two_templates_are_ordered_within_each_item(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, true);
        let nodes = ["name@ada", "other@ada", "name@bob", "other@bob"];
        let bounds = cx.update(|cx| nodes.map(|node| shell::node_bounds(cx, node).unwrap()));
        assert!(
            bounds
                .windows(2)
                .all(|pair| pair[0].origin.x < pair[1].origin.x)
        );
        assert_eq!(f.views.borrow().len(), 4);
    });
}

#[gpui::test]
fn a_key_that_leaves_and_returns_gets_a_new_view_even_without_a_draw_between(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let ada = card(&f, "name@ada");
        let id = ada.entity_id();
        drop(ada);
        cx.update(|cx| {
            f.model.update(cx, |names, cx| {
                names.0.remove(0);
                cx.notify();
            })
        });
        assert!(
            f.views
                .borrow()
                .get("name@ada")
                .and_then(gpui::WeakEntity::upgrade)
                .is_none()
        );
        cx.update(|cx| {
            f.model.update(cx, |names, cx| {
                names.0.insert(0, "ada".into());
                cx.notify();
            })
        });
        draw(f.window, cx);
        assert_ne!(card(&f, "name@ada").entity_id(), id);
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

#[gpui::test]
fn invalid_and_repeated_keys_warn_once_per_key_and_are_skipped(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        for _ in 0..3 {
            cx.update(|cx| {
                f.model.update(cx, |names, cx| {
                    names.0 = vec![
                        "ada".into(),
                        "bob".into(),
                        "bob".into(),
                        "Ada".into(),
                        "".into(),
                    ];
                    cx.notify();
                })
            });
            draw(f.window, cx);
        }
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_some());
        assert!(cx.update(|cx| shell::item(cx, "name@Ada")).is_none());
        assert_eq!(
            LOGS.lock().unwrap().len(),
            3,
            "one item slot warns once for three bad keys"
        );
        assert_eq!(f.views.borrow().len(), 2);
    });
}

#[gpui::test]
fn owner_model_takes_precedence_over_window_model(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        cx.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let model = cx.new(|_| Names(vec!["window".into()]));
            host.provide_window(cx, model);
            host.register(cx, "tour.frame", &["children"], |_, slots, _, cx| {
                cx.new(|_| Frame(slots)).into()
            });
            host.provide::<Names>(cx, "tour.frame", |_, cx| {
                cx.new(|_| Names(vec!["owner".into()]))
            });
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    count: 0,
                    titles: Rc::default(),
                })
                .into()
            });
            host.place_root(cx, "root", "tour.frame");
            host.item_slot::<Names>(cx, "root", "children");
            host.place(cx, "name", "tour.card", "root", "children");
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        draw(window, cx);
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@owner")),
            Some("owner".into())
        );
        assert!(cx.update(|cx| shell::item(cx, "name@window")).is_none());
    });
}

#[gpui::test]
fn copies_named_as_operation_parents_or_position_anchors_are_refused(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let copy =
            yard_layout::NodeId::copy(&yard_layout::NodeId::parse("name").unwrap(), "ada").unwrap();
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| {
                let mark = TRACES.lock().unwrap().len();
                f.host
                    .add(window, cx, "new", "tour.card", "name@ada", "children");
                assert_copy_info(mark, "name@ada");
                let mark = TRACES.lock().unwrap().len();
                f.host.add_at(
                    window,
                    cx,
                    "other",
                    "tour.card",
                    "left",
                    "children",
                    yard_layout::Position::Before(copy.clone()),
                );
                assert_copy_info(mark, "name@ada");
                let mark = TRACES.lock().unwrap().len();
                f.host.move_node(cx, "name", "name@ada", "children", 0);
                assert_copy_info(mark, "name@ada");
                let mark = TRACES.lock().unwrap().len();
                f.host.move_to(
                    cx,
                    "name",
                    "left",
                    "children",
                    yard_layout::Position::After(copy),
                );
                assert_copy_info(mark, "name@ada");
            })
            .unwrap();
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(f.views.borrow().len(), 2);
    });
}

#[gpui::test]
fn moving_an_existing_container_into_an_item_slot_drops_original_descendant_views(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| {
                f.host
                    .add(window, cx, "frame", "tour.frame", "root", "children");
                f.host
                    .add(window, cx, "child", "tour.card", "frame", "children");
            })
            .unwrap();
        let child = card(&f, "child").downgrade();
        cx.update(|cx| f.host.move_node(cx, "frame", "left", "children", 0));
        draw(f.window, cx);
        assert!(child.upgrade().is_none());
        assert_eq!(
            cx.update(|cx| shell::item(cx, "frame@ada")),
            Some("ada".into())
        );
    });
}

#[gpui::test]
fn removing_a_template_drops_its_copies_before_another_draw(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let ada = card(&f, "name@ada").downgrade();
        let bob = card(&f, "name@bob").downgrade();
        cx.update(|cx| f.host.remove(cx, "name"));
        assert!(ada.upgrade().is_none());
        assert!(bob.upgrade().is_none());
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_none());
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
    });
}

fn assert_copy_info(since: usize, node: &str) {
    let traces = TRACES.lock().unwrap();
    assert!(
        traces[since..].iter().any(|line| line.contains(": INFO")
            && line.contains(&format!("{node} is a copy of an item slot"))),
        "{:#?}",
        &traces[since..]
    );
}

#[gpui::test]
fn focused_copy_actions_refuse_with_info(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let mut visual = gpui::VisualTestContext::from_window(f.window.into(), cx);
        let position = visual.debug_bounds("CARD-name@ada").unwrap().center();
        visual.simulate_click(position, gpui::Modifiers::default());
        assert_eq!(
            visual.update(|window, cx| shell::focused_node(window, cx).unwrap().to_string()),
            "name@ada"
        );
        for action in [
            Box::new(shell::ToggleZoom) as Box<dyn gpui::Action>,
            Box::new(shell::Hide),
        ] {
            let mark = TRACES.lock().unwrap().len();
            visual.update(|window, cx| window.dispatch_action(action, cx));
            assert_copy_info(mark, "name@ada");
        }
        let mark = TRACES.lock().unwrap().len();
        cx.update(|cx| f.host.set_zoomed(cx, "name@ada"));
        assert_copy_info(mark, "name@ada");
        assert!(cx.update(|cx| shell::zoomed(cx)).is_none());
        assert!(!cx.update(|cx| shell::is_hidden(cx, "name@ada")));
        cx.update(|cx| f.host.set_hidden(cx, "name", true));
        assert!(cx.update(|cx| shell::is_hidden(cx, "name@ada")));
        assert!(LOGS.lock().unwrap().is_empty());
    });
}

#[gpui::test]
fn focused_copy_can_toggle_out_of_its_zoomed_row(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        cx.update(|cx| f.host.set_zoomed(cx, "left"));
        draw(f.window, cx);
        let mut visual = gpui::VisualTestContext::from_window(f.window.into(), cx);
        let position = visual.debug_bounds("CARD-name@ada").unwrap().center();
        visual.simulate_click(position, gpui::Modifiers::default());
        assert_eq!(
            visual.update(|window, cx| shell::focused_node(window, cx).unwrap().to_string()),
            "name@ada"
        );
        assert_eq!(
            cx.update(|cx| shell::zoomed(cx).unwrap().to_string()),
            "left"
        );
        visual.update(|window, cx| window.dispatch_action(Box::new(shell::ToggleZoom), cx));
        assert!(cx.update(|cx| shell::zoomed(cx)).is_none());
    });
}

#[gpui::test]
fn malformed_at_ids_log_errors_instead_of_copy_refusals(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let mark = TRACES.lock().unwrap().len();
        cx.update(|cx| {
            f.host.set_zoomed(cx, "x@Bad");
            f.host.set_hidden(cx, "x@Bad", true);
        });
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| {
                f.host
                    .add(window, cx, "new", "tour.card", "x@Bad", "children");
                f.host.move_node(cx, "name", "x@Bad", "children", 0);
            })
            .unwrap();
        // Position anchors are NodeIds: malformed IDs are refused before a
        // Before or After position can be constructed.
        assert!(yard_layout::NodeId::parse("x@Bad").is_err());
        let logs = LOGS.lock().unwrap();
        assert_eq!(logs.len(), 4, "{logs:?}");
        assert!(
            logs[..2]
                .iter()
                .all(|line| line.contains("invalid node ID") && line.contains("x@Bad"))
        );
        assert!(
            logs[2..]
                .iter()
                .all(|line| line.contains("invalid parent ID") && line.contains("x@Bad"))
        );
        assert!(
            !TRACES.lock().unwrap()[mark..]
                .iter()
                .any(|line| line.contains("is a copy of an item slot"))
        );
    });
}

#[gpui::test]
fn observer_creates_and_drops_copies_without_drawing(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let ada = card(&f, "name@ada").downgrade();
        cx.update(|cx| {
            f.model.update(cx, |names, cx| {
                names.0 = vec!["dee".into(), "bob".into()];
                cx.notify();
            })
        });
        assert!(ada.upgrade().is_none());
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_none());
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@dee")),
            Some("dee".into())
        );
        assert!(card(&f, "name@dee").entity_id() != card(&f, "name@bob").entity_id());
    });
}

#[gpui::test]
fn reset_reconciles_copies_and_items_before_drawing(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        cx.update(|cx| f.host.move_node(cx, "name", "right", "children", 0));
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_none());
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| f.host.reset_layout(window, cx))
            .unwrap();
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@ada")),
            Some("ada".into())
        );
        assert!(card(&f, "name@ada").downgrade().upgrade().is_some());
        cx.update(|cx| f.host.set_hidden(cx, "name", true));
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| f.host.reset_layout(window, cx))
            .unwrap();
        assert!(!cx.update(|cx| shell::is_hidden(cx, "name@bob")));
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

#[gpui::test]
fn removing_an_owner_then_resetting_restores_copies_and_observer_without_drawing(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let old = card(&f, "name@ada").downgrade();
        cx.update(|cx| f.host.remove(cx, "left"));
        assert!(old.upgrade().is_none());
        assert!(cx.update(|cx| shell::item(cx, "name@ada")).is_none());
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| f.host.reset_layout(window, cx))
            .unwrap();
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@ada")),
            Some("ada".into())
        );
        assert!(card(&f, "name@bob").downgrade().upgrade().is_some());
        cx.update(|cx| {
            f.model.update(cx, |names, cx| {
                names.0.push("cy".into());
                cx.notify();
            })
        });
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@cy")),
            Some("cy".into())
        );
        assert!(card(&f, "name@cy").downgrade().upgrade().is_some());
    });
}

#[gpui::test]
fn column_item_slot_orders_its_copies_vertically(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| {
                f.host
                    .add(window, cx, "column", "yard.column", "root", "children");
                f.host.item_slot::<Names>(cx, "column", "children");
                f.host
                    .add(window, cx, "column_card", "tour.card", "column", "children");
            })
            .unwrap();
        draw(f.window, cx);
        let [ada, bob] = cx.update(|cx| {
            ["column_card@ada", "column_card@bob"].map(|node| shell::node_bounds(cx, node).unwrap())
        });
        assert_eq!(ada.origin.x, bob.origin.x);
        assert!(ada.origin.y < bob.origin.y);
        assert_eq!(
            cx.update(|cx| shell::item(cx, "column_card@bob")),
            Some("bob".into())
        );
    });
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct CardProps {
    value: usize,
}
struct PropCard {
    props: CardProps,
}
impl gpui::Render for PropCard {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}
impl shell::PropsView<CardProps> for PropCard {
    fn set_props(&mut self, props: CardProps, _: &mut gpui::Context<Self>) {
        self.props = props;
    }
}

#[gpui::test]
fn template_props_reach_kept_copies_without_drawing(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let props = Rc::new(RefCell::new(
            BTreeMap::<String, gpui::WeakEntity<PropCard>>::new(),
        ));
        cx.update(|cx| {
            let props = props.clone();
            f.host
                .register_with_props(cx, "tour.prop", &[], move |initial, node, _, _, cx| {
                    props
                        .borrow_mut()
                        .insert(node.to_string(), cx.entity().downgrade());
                    PropCard { props: initial }
                });
        });
        gpui::AnyWindowHandle::from(f.window)
            .update(cx, |_, window, cx| {
                f.host.add_with_props(
                    window,
                    cx,
                    "prop",
                    "tour.prop",
                    "left",
                    "children",
                    CardProps { value: 3 },
                );
            })
            .unwrap();
        let ada = props.borrow()["prop@ada"].upgrade().unwrap();
        let bob = props.borrow()["prop@bob"].upgrade().unwrap();
        assert_eq!(cx.update(|cx| ada.read(cx).props.value), 3);
        cx.update(|cx| f.host.set_props(cx, "prop", CardProps { value: 8 }));
        assert_eq!(
            props.borrow()["prop@ada"].upgrade().unwrap().entity_id(),
            ada.entity_id()
        );
        assert_eq!(cx.update(|cx| ada.read(cx).props.value), 8);
        assert_eq!(cx.update(|cx| bob.read(cx).props.value), 8);
    });
}

#[gpui::test]
fn items_leave_the_layout_file_unchanged(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        let path =
            std::env::temp_dir().join(format!("yard-items-rules-{}.json", std::process::id()));
        let original = "{\"version\":1,\"operations\":[]}";
        std::fs::write(&path, original).unwrap();
        cx.update(|cx| shell::set_layout_file(cx, path.clone()));
        for keys in [vec!["bob", "dee"], vec!["dee", "bob"], vec![]] {
            cx.update(|cx| {
                f.model.update(cx, |names, cx| {
                    names.0 = keys.into_iter().map(Into::into).collect();
                    cx.notify();
                })
            });
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
        }
        std::fs::remove_file(path).unwrap();
    });
}

#[gpui::test]
fn restore_has_copies_and_item_keys_before_the_first_draw(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, false, false, false);
        cx.update(|cx| f.host.move_node(cx, "left", "right", "children", 0));
        let path =
            std::env::temp_dir().join(format!("yard-items-restore-{}.json", std::process::id()));
        cx.update(|cx| shell::set_layout_file(cx, path.clone()));
        // Save a real operation after choosing the file.
        cx.update(|cx| f.host.set_hidden(cx, "right", true));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains('@'));
        let fresh = cx.new_app();
        let made = Views::default();
        fresh.update(|cx| {
            yard_ui::init(cx);
            shell::set_layout_file(cx, path.clone());
            let host = shell::Extension::new(cx, "tour").unwrap();
            let names = cx.new(|_| Names(vec!["ada".into(), "bob".into()]));
            host.provide_window(cx, names);
            host.register(cx, "tour.frame", &["children"], |_, slots, _, cx| {
                cx.new(|_| Frame(slots)).into()
            });
            let made = made.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                let card = cx.new(|_| Card {
                    node: node.to_string(),
                    count: 0,
                    titles: Rc::default(),
                });
                made.borrow_mut().insert(node.to_string(), card.downgrade());
                card.into()
            });
            host.place_root(cx, "root", "tour.frame");
            host.place(cx, "left", "yard.row", "root", "children");
            host.place(cx, "right", "yard.row", "root", "children");
            host.item_slot::<Names>(cx, "left", "children");
            host.place(cx, "name", "tour.card", "left", "children");
        });
        let _window = fresh.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        assert_eq!(fresh.update(|cx| shell::user_layer_len(cx)), 2);
        assert_eq!(
            fresh.update(|cx| shell::item(cx, "name@ada")),
            Some("ada".into())
        );
        assert!(made.borrow()["name@bob"].upgrade().is_some());
        assert!(fresh.update(|cx| shell::is_hidden(cx, "right")));
        std::fs::remove_file(path).unwrap();
    });
}

#[gpui::test]
fn failed_view_creation_logs_once_across_frames_and_notifications(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let (host, model) = cx.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let model = cx.new(|_| Names(vec!["ada".into(), "bob".into()]));
            host.provide_window(cx, model.clone());
            host.place_root(cx, "root", "yard.row");
            host.item_slot::<Names>(cx, "root", "children");
            host.place(cx, "broken", "tour.unregistered", "root", "children");
            (host, model)
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        let errors = || {
            LOGS.lock()
                .unwrap()
                .iter()
                .filter(|line| line.contains("unregistered component tour.unregistered"))
                .count()
        };
        assert_eq!(errors(), 2, "one failure for each live copy");
        for _ in 0..4 {
            draw(window, cx);
            cx.update(|cx| model.update(cx, |_, cx| cx.notify()));
        }
        assert_eq!(errors(), 2);
        cx.update(|cx| host.set_hidden(cx, "broken", true));
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        assert_eq!(
            errors(),
            4,
            "reset retries both failed copies and logs each failure again"
        );
        draw(window, cx);
        cx.update(|cx| model.update(cx, |_, cx| cx.notify()));
        assert_eq!(errors(), 4);
        let created = Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let created = created.clone();
            host.register(cx, "tour.unregistered", &[], move |_, _, _, cx| {
                created.set(created.get() + 1);
                cx.new(|_| gpui::Empty).into()
            });
        });
        assert_eq!(
            created.get(),
            2,
            "registration retries failed copies without a draw"
        );
        assert_eq!(errors(), 4);
    });
}

#[gpui::test]
fn moving_a_container_retries_failed_descendant_views(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let host = cx.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.place_root(cx, "root", "yard.row");
            host.place(cx, "frame", "yard.row", "root", "children");
            host.place(cx, "right", "yard.row", "root", "children");
            host.place(cx, "broken", "tour.unregistered", "frame", "children");
            let model = cx.new(|_| Names(Vec::new()));
            host.provide_window(cx, model);
            host.place(cx, "items", "yard.row", "root", "children");
            host.item_slot::<Names>(cx, "items", "children");
            host
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        let errors = || {
            LOGS.lock()
                .unwrap()
                .iter()
                .filter(|line| line.contains("unregistered component tour.unregistered"))
                .count()
        };
        assert_eq!(errors(), 1);
        cx.update(|cx| host.move_node(cx, "frame", "right", "children", 0));
        draw(window, cx);
        assert_eq!(
            errors(),
            2,
            "moving the ancestor retries the failed descendant"
        );
    });
}

#[gpui::test]
fn dragging_a_copy_refuses_the_move_with_info(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        let f = setup(cx, true, false, false);
        let ada = card(&f, "name@ada");
        let mut visual = gpui::VisualTestContext::from_window(f.window.into(), cx);
        let start = visual.debug_bounds("TAB-name@ada").unwrap().center();
        let to = visual.debug_bounds("TAB-name@bob").unwrap().center();
        let mark = TRACES.lock().unwrap().len();
        visual.simulate_mouse_move(start, None, Default::default());
        visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            gpui::point(start.x + px(5.), start.y),
            gpui::MouseButton::Left,
            Default::default(),
        );
        draw(f.window, cx);
        visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
        draw(f.window, cx);
        visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
        assert_copy_info(mark, "name@ada");
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
        assert_eq!(card(&f, "name@ada").entity_id(), ada.entity_id());
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@ada")),
            Some("ada".into())
        );
    });
}

struct CountedNames {
    reads: Rc<std::cell::Cell<usize>>,
}
impl shell::Items for CountedNames {
    fn items(&self) -> Vec<SharedString> {
        self.reads.set(self.reads.get() + 1);
        vec!["ada".into()]
    }
}

#[gpui::test]
fn observers_belong_to_each_window_and_drop_with_the_window_or_slot(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        for close_first in [true, false] {
            let mut app = cx.new_app();
            let reads = Rc::new(std::cell::Cell::new(0));
            let (host, model) = app.update(|cx| {
                yard_ui::init(cx);
                let host = shell::Extension::new(cx, "tour").unwrap();
                let model = cx.new(|_| CountedNames {
                    reads: reads.clone(),
                });
                host.provide_window(cx, model.clone());
                host.place_root(cx, "root", "yard.row");
                host.place(cx, "row", "yard.row", "root", "children");
                host.item_slot::<CountedNames>(cx, "row", "children");
                (host, model)
            });
            let first = app.update(|cx| {
                cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                    .unwrap()
            });
            let second = app.update(|cx| {
                cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                    .unwrap()
            });
            reads.set(0);
            app.update(|cx| model.update(cx, |_, cx| cx.notify()));
            assert_eq!(reads.get(), 2, "both windows own an observer");
            let closing = if close_first { first } else { second };
            gpui::AnyWindowHandle::from(closing)
                .update(&mut app, |_, window, _| window.remove_window())
                .unwrap();
            reads.set(0);
            app.update(|cx| model.update(cx, |_, cx| cx.notify()));
            assert_eq!(reads.get(), 1, "the surviving window keeps its observer");
            app.update(|cx| host.remove(cx, "row"));
            reads.set(0);
            app.update(|cx| model.update(cx, |_, cx| cx.notify()));
            assert_eq!(
                reads.get(),
                0,
                "removing the owner drops the remaining observer"
            );
        }
    });
}

struct OrderedFrame(shell::Slots);
impl gpui::Render for OrderedFrame {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut root = gpui::div().size_full().relative();
        for (slot, x) in [("source", 0.), ("z", 200.), ("a", 200.)] {
            for view in self.0.views(slot) {
                root = root.child(
                    gpui::div()
                        .absolute()
                        .left(px(x))
                        .top_0()
                        .w(px(100.))
                        .h_full()
                        .child(view),
                );
            }
        }
        root
    }
}

#[gpui::test]
fn drawn_walk_uses_tree_slot_order_instead_of_sorting_slot_names(cx: &mut TestAppContext) {
    captured(cx, |cx| {
        cx.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(
                cx,
                "tour.ordered",
                &["source", "z", "a"],
                |_, slots, _, cx| cx.new(|_| OrderedFrame(slots)).into(),
            );
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    count: 0,
                    titles: Rc::default(),
                })
                .into()
            });
            host.place_root(cx, "root", "tour.ordered");
            host.place(cx, "source", "tour.card", "root", "source");
            // These placements make the tree's slot order source, z, a.
            host.place(cx, "first", "tour.card", "root", "z");
            host.place(cx, "second", "tour.card", "root", "a");
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            shell::node_focus_handle(cx, "source")
                .unwrap()
                .focus(window, cx)
        });
        visual.dispatch_action(shell::FocusRight);
        assert_eq!(
            visual.update(|window, cx| shell::focused_node(window, cx).unwrap().to_string()),
            "first"
        );
    });
}

#[gpui::test]
fn late_owner_registration_resolves_its_newly_created_model_without_a_draw(
    cx: &mut TestAppContext,
) {
    captured(cx, |cx| {
        let views = Views::default();
        let host = cx.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.provide::<Names>(cx, "tour.late", |_, cx| {
                cx.new(|_| Names(vec!["own".into()]))
            });
            let views = views.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                let view = cx.new(|_| Card {
                    node: node.to_string(),
                    count: 0,
                    titles: Rc::default(),
                });
                views
                    .borrow_mut()
                    .insert(node.to_string(), view.downgrade());
                view.into()
            });
            host.place_root(cx, "root", "tour.late");
            host.item_slot::<Names>(cx, "root", "children");
            host.place(cx, "name", "tour.card", "root", "children");
            host
        });
        let _window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        assert!(cx.update(|cx| shell::item(cx, "name@own")).is_none());
        cx.update(|cx| {
            host.register(cx, "tour.late", &["children"], |_, slots, _, cx| {
                cx.new(|_| Frame(slots)).into()
            })
        });
        assert_eq!(
            cx.update(|cx| shell::item(cx, "name@own")),
            Some("own".into())
        );
        assert!(views.borrow()["name@own"].upgrade().is_some());
    });
}
