#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, ParentElement, SharedString, Styled, TestAppContext};
use riso_shell::dev as shell;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);
#[derive(Default)]
struct Captured {
    spans: std::collections::BTreeMap<u64, String>,
    entered: Vec<u64>,
    groups: Vec<(String, Vec<String>)>,
    events: Vec<(String, Vec<String>)>,
}
#[derive(Default)]
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
}
impl Captured {
    fn ancestors(&self) -> Vec<String> {
        self.entered
            .iter()
            .map(|id| self.spans[id].clone())
            .collect()
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
        let name = attributes.metadata().name().to_owned();
        let ancestors = captured.ancestors();
        captured.groups.push((name.clone(), ancestors));
        captured.spans.insert(id, name);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let ancestors = captured.ancestors();
        captured.events.push((fields.0, ancestors));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

fn host(app: &mut TestAppContext) -> shell::Extension {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.card", &[], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        });
        host.place_root(cx, "root", "riso.row");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "b", "tour.card", "root", "children");
        host
    })
}

fn open(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    app.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    })
}

#[gpui::test]
fn moving_a_default_node_keeps_its_origin(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let host = host(&mut app);
    let _window = open(&mut app);
    app.update(|cx| host.move_node(cx, "b", "root", "children", 0));
    let nodes = app.update(|cx| shell::inspect(cx));
    assert_eq!(nodes[1].id, "b");
    assert_eq!(nodes[1].origin, shell::Origin::Default);
}

#[gpui::test]
fn a_named_node_leaves_on_default_and_readded_ids_belong_to_their_layer(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let host = host(&mut app);
    app.update(|cx| {
        host.named_layout(cx, "tour.extra", |layout| {
            layout.remove("a");
            layout.add("a", "tour.card", "root", "children");
            layout.add("extra", "tour.card", "root", "children");
        })
    });
    let window = open(&mut app);
    window
        .update(&mut app, |_, window, cx| {
            host.show_layout(window, cx, "tour.extra")
        })
        .unwrap();
    let nodes = app.update(|cx| shell::inspect(cx));
    for id in ["a", "extra"] {
        let node = nodes.iter().find(|node| node.id == id).unwrap();
        assert_eq!(node.origin, shell::Origin::NamedLayout("tour.extra".into()));
    }
    app.update(|cx| host.remove(cx, "extra"));
    window
        .update(&mut app, |_, window, cx| {
            host.add(window, cx, "extra", "tour.card", "root", "children")
        })
        .unwrap();
    assert_eq!(
        app.update(|cx| shell::inspect(cx)).last().unwrap().origin,
        shell::Origin::UserLayer
    );
    window
        .update(&mut app, |_, window, cx| {
            host.show_layout(window, cx, "riso.default")
        })
        .unwrap();
    let nodes = app.update(|cx| shell::inspect(cx));
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["root", "a", "b"]
    );
    assert!(
        nodes
            .iter()
            .all(|node| node.origin == shell::Origin::Default)
    );
}

struct Names;
impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        vec!["ada".into()]
    }
}
struct Container(shell::Slots);
impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().children(self.0.views("children"))
    }
}

#[gpui::test]
fn copies_follow_the_templates_own_hidden_flag(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let host = app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.names", &["children"], |_, slots, _, cx| {
            cx.new(|_| Container(slots)).into()
        });
        host.provide::<Names>(cx, "tour.names", |_, cx| cx.new(|_| Names));
        host.register(cx, "tour.card", &[], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        });
        host.place_root(cx, "root", "tour.names");
        host.place(cx, "row", "riso.row", "root", "children");
        host.item_slot::<Names>(cx, "row", "children");
        host.place(cx, "name", "tour.card", "row", "children");
        host
    });
    let _window = open(&mut app);
    for hidden in [true, false, true, false] {
        app.update(|cx| host.set_hidden(cx, "name", hidden));
        let nodes = app.update(|cx| shell::inspect(cx));
        let copy = nodes.iter().find(|node| node.id == "name@ada").unwrap();
        assert_eq!(copy.hidden, hidden);
        assert_eq!(copy.origin, shell::Origin::Copy("ada".into()));
        assert_eq!(copy.depth, 2);
    }
}

#[gpui::test]
fn subscriptions_for_two_windows_can_be_dropped_in_any_order(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    for order in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]] {
        let mut app = cx.new_app();
        let host = host(&mut app);
        let _one = open(&mut app);
        let _two = open(&mut app);
        let calls: Vec<_> = (0..4).map(|_| Rc::new(Cell::new(0))).collect();
        let mut subscriptions: Vec<_> = calls
            .iter()
            .map(|calls| {
                let calls = calls.clone();
                Some(
                    app.update(|cx| shell::observe_layout(cx, move |_| calls.set(calls.get() + 1))),
                )
            })
            .collect();
        let mut expected = [0; 4];
        for drop_index in order {
            subscriptions[drop_index].take();
            app.update(|cx| host.set_hidden(cx, "a", true));
            for (index, subscription) in subscriptions.iter().enumerate() {
                expected[index] += usize::from(subscription.is_some());
                assert_eq!(calls[index].get(), expected[index]);
            }
        }
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let _replacement =
            app.update(|cx| shell::observe_layout(cx, move |_| counter.set(counter.get() + 1)));
        app.update(|cx| host.set_hidden(cx, "a", false));
        assert_eq!(calls.get(), 1);
    }
}

#[gpui::test]
fn registering_a_component_without_changing_the_tree_does_not_notify(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let host = host(&mut app);
    let _window = open(&mut app);
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let _subscription =
        app.update(|cx| shell::observe_layout(cx, move |_| counter.set(counter.get() + 1)));
    app.update(|cx| {
        host.register(cx, "tour.other", &[], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        })
    });
    assert_eq!(calls.get(), 0);
}

#[gpui::test]
fn restore_notifies_once_with_the_final_item_copies(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let path = std::env::temp_dir().join(format!("riso-inspect-rules-{}.json", std::process::id()));
    std::fs::write(
        &path,
        r#"{"version":1,"operations":[{"op":"hide","node":"a"},{"op":"add","node":"restored","component":"tour.card","extension":"tour","parent":"root","slot":"children"}]}"#,
    )
    .unwrap();
    app.update(|cx| {
        riso_ui::init(cx);
        shell::set_layout_file(cx, path.clone());
        let host = shell::Extension::new(cx, "tour").unwrap();
        host.register(cx, "tour.names", &["children"], |_, slots, _, cx| {
            cx.new(|_| Container(slots)).into()
        });
        host.provide::<Names>(cx, "tour.names", |_, cx| cx.new(|_| Names));
        host.register(cx, "tour.card", &[], |_, _, _, cx| {
            cx.new(|_| gpui::Empty).into()
        });
        host.place_root(cx, "root", "tour.names");
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "row", "riso.row", "root", "children");
        host.item_slot::<Names>(cx, "row", "children");
        host.place(cx, "name", "tour.card", "row", "children");
    });
    let calls = Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = calls.clone();
    let _subscription = app.update(|cx| {
        shell::observe_layout(cx, move |cx| recorded.borrow_mut().push(shell::inspect(cx)))
    });
    let _window = open(&mut app);
    std::fs::remove_file(path).unwrap();
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].iter().find(|node| node.id == "a").unwrap().hidden);
    assert!(calls[0].iter().any(|node| node.id == "name@ada"));
    assert_eq!(
        calls[0]
            .iter()
            .find(|node| node.id == "restored")
            .unwrap()
            .origin,
        shell::Origin::UserLayer
    );
    assert_eq!(
        calls[0].iter().find(|node| node.id == "a").unwrap().origin,
        shell::Origin::Default
    );
    assert_eq!(
        calls[0]
            .iter()
            .find(|node| node.id == "name@ada")
            .unwrap()
            .origin,
        shell::Origin::Copy("ada".into())
    );
}

struct Card {
    node: String,
    slots: shell::Slots,
}
impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::InteractiveElement;
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
            .children(self.slots.views("body"))
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

#[gpui::test]
fn split_observers_see_only_the_completed_transaction_and_no_rollback(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        for rollback in [false, true] {
            let mut app = cx.new_app();
            app.update(|cx| {
                riso_ui::init(cx);
                let host = shell::Extension::new(cx, "tour").unwrap();
                host.register(cx, "tour.card", &["body"], |node, slots, _, cx| {
                    cx.new(|_| Card {
                        node: node.to_string(),
                        slots,
                    })
                    .into()
                });
                if rollback {
                    host.place_root(cx, "root", "riso.tabs");
                    host.place(cx, "branch", "tour.card", "root", "children");
                    host.place(cx, "other", "tour.card", "root", "children");
                    host.place(cx, "inner", "riso.column", "branch", "body");
                    host.place(cx, "pad", "tour.card", "inner", "children");
                    host.place(cx, "leaf", "tour.card", "inner", "children");
                } else {
                    host.place_root(cx, "root", "riso.row");
                    host.place(cx, "tabs", "riso.tabs", "root", "children");
                    host.place(cx, "b", "tour.card", "tabs", "children");
                    host.place(cx, "target", "tour.card", "root", "children");
                }
            });
            let window = open(&mut app);
            app.simulate_window_resize(window.into(), gpui::size(gpui::px(1200.), gpui::px(700.)));
            window
                .update(&mut app, |_, window, _| window.refresh())
                .unwrap();
            draw(window, &mut app);
            let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
            let calls = Rc::new(std::cell::RefCell::new(Vec::new()));
            let recorded = calls.clone();
            let _subscription = app.update(|cx| {
                shell::observe_layout(cx, move |cx| recorded.borrow_mut().push(shell::inspect(cx)))
            });
            let start = visual
                .debug_bounds(if rollback { "TAB-branch" } else { "TAB-b" })
                .unwrap()
                .center();
            let target = visual
                .debug_bounds(if rollback { "CARD-leaf" } else { "CARD-target" })
                .unwrap();
            let to = if rollback {
                gpui::point(target.left() + gpui::px(8.), target.center().y)
            } else {
                gpui::point(target.center().x, target.top() + gpui::px(8.))
            };
            visual.simulate_mouse_move(start, None, Default::default());
            visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
            visual.simulate_mouse_move(
                gpui::point(start.x + gpui::px(5.), start.y),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, &mut app);
            visual.simulate_mouse_move(
                gpui::point((start.x + to.x) * 0.5, (start.y + to.y) * 0.5),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_move(to, gpui::MouseButton::Left, Default::default());
            draw(window, &mut app);
            assert!(calls.borrow().is_empty());
            visual.simulate_mouse_up(to, gpui::MouseButton::Left, Default::default());
            draw(window, &mut app);
            if rollback {
                assert!(
                    trace
                        .0
                        .lock()
                        .unwrap()
                        .events
                        .iter()
                        .any(|(fields, _)| fields.contains("split on drop undone")
                            && fields.contains("done=3")),
                    "the drop must reach and undo its partially applied transaction"
                );
                assert!(calls.borrow().is_empty());
                assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
            } else {
                assert_eq!(calls.borrow().len(), 1);
                let calls = calls.borrow();
                let b = calls[0].iter().find(|node| node.id == "b").unwrap();
                let tabs = calls[0].iter().find(|node| node.id == "tabs-1").unwrap();
                assert_eq!(b.depth, tabs.depth + 1);
                assert_eq!(tabs.origin, shell::Origin::UserLayer);
                assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 4);
            }
        }
    });
}

#[gpui::test]
fn focus_alone_does_not_notify(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let _host = host(&mut app);
    let window = open(&mut app);
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let _subscription =
        app.update(|cx| shell::observe_layout(cx, move |_| counter.set(counter.get() + 1)));
    for id in ["a", "b", "a"] {
        window
            .update(&mut app, |_, window, cx| {
                shell::focus_handle(cx, id).unwrap().focus(window, cx);
                assert_eq!(shell::focused_node(window, cx).unwrap().as_str(), id);
            })
            .unwrap();
        assert_eq!(calls.get(), 0);
    }
}

#[gpui::test]
fn reset_removes_user_origins_and_preserves_named_origins(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let mut app = cx.new_app();
    let host = host(&mut app);
    app.update(|cx| {
        host.named_layout(cx, "tour.extra", |layout| {
            layout.add("extra", "tour.card", "root", "children")
        })
    });
    let window = open(&mut app);
    window
        .update(&mut app, |_, window, cx| {
            host.show_layout(window, cx, "tour.extra");
            host.add(window, cx, "user", "tour.card", "root", "children");
        })
        .unwrap();
    assert_eq!(
        app.update(|cx| shell::inspect(cx))
            .iter()
            .find(|node| node.id == "user")
            .unwrap()
            .origin,
        shell::Origin::UserLayer
    );
    app.update(|cx| host.set_hidden(cx, "a", true));
    window
        .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
        .unwrap();
    let nodes = app.update(|cx| shell::inspect(cx));
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["root", "a", "b", "extra"]
    );
    assert!(!nodes.iter().find(|node| node.id == "a").unwrap().hidden);
    assert_eq!(
        nodes.iter().find(|node| node.id == "a").unwrap().origin,
        shell::Origin::Default
    );
    assert_eq!(
        nodes.iter().find(|node| node.id == "extra").unwrap().origin,
        shell::Origin::NamedLayout("tour.extra".into())
    );
}

struct ChangingNames(Vec<SharedString>);
impl shell::Items for ChangingNames {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

#[gpui::test]
fn one_notify_changes_two_sibling_item_groups_and_notifies_once(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let model = app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let model = cx.new(|_| ChangingNames(vec!["ada".into()]));
            host.register(cx, "tour.names", &["children"], |_, slots, _, cx| {
                cx.new(|_| Container(slots)).into()
            });
            let provided = model.clone();
            host.provide::<ChangingNames>(cx, "tour.names", move |_, _| provided.clone());
            host.register(cx, "tour.card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            host.place_root(cx, "root", "tour.names");
            for (row, name) in [("one", "first"), ("two", "second")] {
                host.place(cx, row, "riso.row", "root", "children");
                host.item_slot::<ChangingNames>(cx, row, "children");
                host.place(cx, name, "tour.card", row, "children");
            }
            model
        });
        let _window = open(&mut app);
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let _subscription = app.update(|cx| {
            shell::observe_layout(cx, move |cx| {
                count.set(count.get() + 1);
                let nodes = shell::inspect(cx);
                for id in ["first@bea", "second@bea"] {
                    assert!(nodes.iter().any(|node| node.id == id));
                }
            })
        });
        trace.0.lock().unwrap().groups.clear();
        trace.0.lock().unwrap().events.clear();
        app.update(|cx| {
            model.update(cx, |model, cx| {
                model.0.push("bea".into());
                cx.notify();
            })
        });
        assert_eq!(calls.get(), 1);
        let captured = trace.0.lock().unwrap();
        let groups = captured
            .groups
            .iter()
            .filter(|(name, _)| name == "items-changed")
            .collect::<Vec<_>>();
        assert_eq!(groups.len(), 2);
        for (_, ancestors) in groups {
            assert!(
                !ancestors.iter().any(|name| name == "items-changed"),
                "{ancestors:?}"
            );
        }
        let lines = captured
            .events
            .iter()
            .filter(|(fields, _)| fields.contains("layout observers"))
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].1.iter().any(|name| name == "items-changed"));
    });
}

#[gpui::test]
fn startup_components_have_the_same_restore_ancestry_as_production(cx: &mut TestAppContext) {
    let _lock = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let path =
            std::env::temp_dir().join(format!("riso-inspect-ancestry-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"version":1,"operations":[{"op":"hide","node":"a"}]}"#,
        )
        .unwrap();
        let production = cx.new_app();
        production.update(|cx| {
            riso_ui::init(cx);
            riso_shell::set_layout_file(cx, path.clone());
            let host = riso_shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "b", "tour.card", "root", "children");
            cx.open_window(Default::default(), riso_shell::new_shell)
                .unwrap()
        });
        let expected = trace
            .0
            .lock()
            .unwrap()
            .groups
            .iter()
            .filter(|(name, _)| name == "component-create")
            .cloned()
            .collect::<Vec<_>>();
        assert!(!expected.is_empty());
        assert!(
            expected
                .iter()
                .all(|(_, ancestors)| !ancestors.iter().any(|name| name == "layout-restore"))
        );
        trace.0.lock().unwrap().groups.clear();
        let mut app = cx.new_app();
        host(&mut app);
        app.update(|cx| shell::set_layout_file(cx, path.clone()));
        let _window = open(&mut app);
        let nodes = app.update(|cx| shell::inspect(cx));
        assert!(nodes.iter().find(|node| node.id == "a").unwrap().hidden);
        let actual = trace
            .0
            .lock()
            .unwrap()
            .groups
            .iter()
            .filter(|(name, _)| name == "component-create")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        std::fs::remove_file(path).unwrap();
    });
}

struct ObservedPanel {
    _subscription: gpui::Subscription,
}
impl gpui::Render for ObservedPanel {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        use gpui::InteractiveElement;
        gpui::div()
            .size_full()
            .debug_selector(|| "OBSERVED-PANEL".to_owned())
    }
}

#[gpui::test]
fn closing_a_window_or_removing_a_panel_releases_its_view_and_subscription(
    cx: &mut TestAppContext,
) {
    let _lock = LOCK.lock().unwrap();
    for close_window in [false, true] {
        let mut app = cx.new_app();
        let host = host(&mut app);
        let weak = Rc::new(std::cell::RefCell::new(None));
        let panel = weak.clone();
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        app.update(|cx| {
            host.register(cx, "tour.panel", &[], move |_, _, _, cx| {
                let count = count.clone();
                let entity = cx.new(|cx| ObservedPanel {
                    _subscription: shell::observe_layout(cx, move |_| count.set(count.get() + 1)),
                });
                *panel.borrow_mut() = Some(entity.downgrade());
                entity.into()
            });
            host.place(cx, "panel", "tour.panel", "root", "children");
        });
        let window = open(&mut app);
        assert!(weak.borrow().as_ref().unwrap().upgrade().is_some());
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert_eq!(calls.get(), 1);
        if close_window {
            window
                .update(&mut app, |_, window, _| window.remove_window())
                .unwrap();
        } else {
            app.update(|cx| host.remove(cx, "panel"));
        }
        assert!(
            weak.borrow().as_ref().unwrap().upgrade().is_none(),
            "the global view map must release the panel"
        );
        let before = calls.get();
        app.update(|cx| host.set_hidden(cx, "a", false));
        assert_eq!(calls.get(), before);
    }
}

#[gpui::test]
fn shared_and_dynamically_added_panels_live_until_the_last_window_closes_in_either_order(
    cx: &mut TestAppContext,
) {
    let _lock = LOCK.lock().unwrap();
    for (close_first, dynamic) in [(0, false), (1, false), (0, true), (1, true)] {
        let mut app = cx.new_app();
        let host = host(&mut app);
        let weak = Rc::new(std::cell::RefCell::new(None));
        let panel = weak.clone();
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        app.update(|cx| {
            host.register(cx, "tour.panel", &[], move |_, _, _, cx| {
                let count = counter.clone();
                let entity = cx.new(|cx| ObservedPanel {
                    _subscription: shell::observe_layout(cx, move |_| count.set(count.get() + 1)),
                });
                *panel.borrow_mut() = Some(entity.downgrade());
                entity.into()
            });
            if !dynamic {
                host.place(cx, "panel", "tour.panel", "root", "children");
            }
        });
        let windows = [open(&mut app), open(&mut app)];
        if dynamic {
            windows[0]
                .update(&mut app, |_, window, cx| {
                    host.add(window, cx, "panel", "tour.panel", "root", "children");
                })
                .unwrap();
            calls.set(0);
        }
        for window in windows {
            app.simulate_window_resize(window.into(), gpui::size(gpui::px(900.), gpui::px(600.)));
            draw(window, &mut app);
        }
        windows[close_first]
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        assert!(weak.borrow().as_ref().unwrap().upgrade().is_some());
        let survivor = windows[1 - close_first];
        survivor
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        draw(survivor, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(survivor.into(), &app);
        assert!(
            visual.debug_bounds("OBSERVED-PANEL").is_some(),
            "the surviving window still renders the shared child view"
        );
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert_eq!(calls.get(), 1);
        survivor
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        assert!(weak.borrow().as_ref().unwrap().upgrade().is_none());
        app.update(|cx| host.set_hidden(cx, "a", false));
        assert_eq!(calls.get(), 1);
    }
}
