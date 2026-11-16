use gpui::{AppContext, IntoElement, ParentElement, SharedString, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// Every test captures the trace with its own scoped subscriber, one at a
// time, so no test reaches a log line while another listens to it.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    // Each line, with the groups open around it, outermost first.
    lines: Vec<(Vec<String>, String)>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let rendered = format!("{value:?}").replace('"', "");
        write!(self.0, " {}={rendered}", field.name()).unwrap();
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
        // A group's opening is a line too, so a group with no line of its own
        // still shows where it opened.
        let mut stack: Vec<String> = captured
            .entered
            .iter()
            .map(|id| captured.spans[id].clone())
            .collect();
        stack.push(label.clone());
        captured.lines.push((stack, String::from("group opened")));
        captured.spans.insert(id, label);
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let stack = captured
            .entered
            .iter()
            .map(|id| captured.spans[id].clone())
            .collect();
        let line = format!("{}{}", event.metadata().level(), fields.0);
        captured.lines.push((stack, line));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        let mut captured = self.0.lock().unwrap();
        assert_eq!(captured.entered.pop(), Some(id.into_u64()));
    }
}

impl Trace {
    fn mark(&self) -> usize {
        self.0.lock().unwrap().lines.len()
    }

    // True when a line after `from` holds every part of `parts` and was
    // logged inside a group starting with `group` and holding `group_part`.
    fn held(&self, from: usize, group: &str, group_part: &str, parts: &[&str]) -> bool {
        self.0.lock().unwrap().lines[from..]
            .iter()
            .any(|(stack, line)| {
                parts.iter().all(|part| line.contains(part))
                    && stack
                        .iter()
                        .any(|label| label.starts_with(group) && label.contains(group_part))
            })
    }

    // How many lines after `from` hold every part of `parts`.
    fn count(&self, from: usize, parts: &[&str]) -> usize {
        self.0.lock().unwrap().lines[from..]
            .iter()
            .filter(|(_, line)| parts.iter().all(|part| line.contains(part)))
            .count()
    }

    // True when a group starting with `group` opened, after `from`, inside
    // no other group.
    fn top_level(&self, from: usize, group: &str) -> bool {
        self.0.lock().unwrap().lines[from..]
            .iter()
            .any(|(stack, line)| {
                line == "group opened" && stack.len() == 1 && stack[0].starts_with(group)
            })
    }

    // True when a group starting with `inner` opened, after `from`, inside
    // a group starting with `outer` and holding `outer_part`.
    fn nested(&self, from: usize, outer: &str, outer_part: &str, inner: &str) -> bool {
        self.0.lock().unwrap().lines[from..]
            .iter()
            .any(|(stack, _)| {
                let at = stack
                    .iter()
                    .position(|label| label.starts_with(outer) && label.contains(outer_part));
                at.is_some_and(|at| stack[at + 1..].iter().any(|l| l.starts_with(inner)))
            })
    }
}

fn captured(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext, &Trace)) {
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || test(cx, &trace));
}

// A list of names. `close_item` removes the key unless it is kept, and
// `new_item` adds `new-<n>`; while `later`, both only remember what was
// asked until the test calls `apply`, which notifies.
struct Names {
    names: Vec<SharedString>,
    keep: Vec<&'static str>,
    later: bool,
    asked: Vec<SharedString>,
    to_add: Vec<SharedString>,
    added: usize,
}

impl Names {
    fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| SharedString::from(*name)).collect(),
            keep: Vec::new(),
            later: false,
            asked: Vec::new(),
            to_add: Vec::new(),
            added: 0,
        }
    }

    fn apply(&mut self, cx: &mut gpui::Context<Self>) {
        let asked = std::mem::take(&mut self.asked);
        self.names.retain(|name| !asked.contains(name));
        self.names.append(&mut self.to_add);
        cx.notify();
    }
}

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.names.clone()
    }
}

impl shell::ItemTabs for Names {
    fn close_item(
        &mut self,
        key: &SharedString,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.later {
            self.asked.push(key.clone());
            return;
        }
        if !self.keep.contains(&key.as_str()) {
            self.names.retain(|name| name != key);
        }
        cx.notify();
    }

    fn new_item(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.added += 1;
        let key = SharedString::from(format!("new-{}", self.added));
        if self.later {
            self.to_add.push(key);
            return;
        }
        self.names.push(key);
        cx.notify();
    }
}

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

struct Frame(shell::Slots);

impl gpui::Render for Frame {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().flex().flex_row().children(
            self.0
                .views("children")
                .into_iter()
                .map(|view| gpui::div().flex_1().min_w_0().h_full().child(view)),
        )
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

fn open_and_draw(app: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    draw(window, app);
    window
}

// One `riso.tabs` node `tabs` with item tabs over the window's `Names`,
// with one card template `name`.
fn setup(app: &mut TestAppContext, names: Names) -> gpui::Entity<Names> {
    setup_with_host(app, names).1
}

fn setup_with_host(
    app: &mut TestAppContext,
    names: Names,
) -> (shell::Extension, gpui::Entity<Names>) {
    app.update(|cx| {
        riso_ui::init(cx);
        let host = shell::Extension::new(cx, "tour").unwrap();
        let model = cx.new(|_| names);
        host.provide_window(cx, model.clone());
        host.register(cx, "tour.frame", &["children"], |_, slots, _, cx| {
            cx.new(|_| Frame(slots)).into()
        });
        host.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
        host.place_root(cx, "root", "tour.frame");
        host.place(cx, "tabs", "riso.tabs", "root", "children");
        host.item_tabs::<Names>(cx, "tabs");
        host.place(cx, "name", "tour.card", "tabs", "children");
        (host, model)
    })
}

fn add_name(app: &mut TestAppContext, model: &gpui::Entity<Names>, name: &'static str) {
    app.update(|cx| {
        model.update(cx, |names, cx| {
            names.names.push(name.into());
            cx.notify();
        })
    });
}

fn selected(app: &mut TestAppContext, tabs: &str) -> Option<String> {
    app.update(|cx| shell::selected_tab(cx, tabs).map(|node| node.to_string()))
}

// `debug_bounds` takes a selector that lives for the whole test.
fn selector(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

fn click_close(
    visual: &mut gpui::VisualTestContext,
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    copy: &str,
) {
    let tab = visual
        .debug_bounds(selector(format!("TAB-{copy}")))
        .unwrap_or_else(|| panic!("{copy}'s tab draws"))
        .center();
    visual.simulate_mouse_move(tab, None, gpui::Modifiers::default());
    draw(window, app);
    let button = visual
        .debug_bounds(selector(format!("CLOSE-{copy}")))
        .unwrap_or_else(|| panic!("{copy}'s close button draws on hover"))
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    draw(window, app);
}

fn click_new(
    visual: &mut gpui::VisualTestContext,
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    tabs: &str,
) {
    let button = visual
        .debug_bounds(selector(format!("NEW-{tabs}")))
        .unwrap_or_else(|| panic!("{tabs}'s new button draws"))
        .center();
    visual.simulate_click(button, gpui::Modifiers::default());
    draw(window, app);
}

// A model that does not notify during the click: its click's group logs
// `items unchanged` and the tab stays. Its later notify is a group of its
// own, outside the click's, and a key it adds then is still selected.
#[gpui::test]
fn a_model_that_notifies_later_changes_the_tabs_outside_the_click_group(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let model = setup(&mut app, {
            let mut names = Names::new(&["ada", "bob", "cy"]);
            names.later = true;
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        let click = trace.mark();
        click_close(&mut visual, window, &mut app, "name@bob");
        assert_eq!(
            app.update(|cx| model.read(cx).asked.clone()),
            [SharedString::from("bob")]
        );
        assert!(
            visual.debug_bounds("TAB-name@bob").is_some(),
            "the tab stays until the model changes its items"
        );
        assert!(
            trace.held(
                click,
                "item-close",
                "key=bob",
                &["DEBUG", "items unchanged"]
            ),
            "the click's group says the items did not change"
        );
        let click = trace.mark();
        click_new(&mut visual, window, &mut app, "tabs");
        assert_eq!(app.update(|cx| model.read(cx).added), 1);
        assert!(visual.debug_bounds("TAB-name@new-1").is_none());
        assert!(trace.held(click, "item-new", "tabs=tabs", &["items unchanged"]));
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@ada"));

        let later = trace.mark();
        app.update(|cx| model.update(cx, |names, cx| names.apply(cx)));
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("TAB-name@bob").is_none(),
            "the tab goes when the model notifies"
        );
        assert!(visual.debug_bounds("TAB-name@new-1").is_some());
        assert_eq!(
            selected(&mut app, "tabs").as_deref(),
            Some("name@new-1"),
            "the key the later change adds is selected"
        );
        assert!(
            trace.top_level(later, "items-changed"),
            "the later notify's items-changed is a group of its own"
        );
        assert!(!trace.nested(later, "item-close", "", "items-changed"));
        assert!(!trace.nested(later, "item-new", "", "items-changed"));
        assert!(trace.held(
            later,
            "items-changed",
            "removed=bob",
            &["DEBUG", "view dropped", "name@bob"]
        ));
        assert!(trace.held(
            later,
            "items-changed",
            "added=new-1",
            &["DEBUG", "tab selected", "name@new-1"]
        ));

        // The selection waits for one added key only: the next one is not
        // selected.
        app.update(|cx| {
            model.update(cx, |names, cx| {
                names.names.push("dee".into());
                cx.notify();
            })
        });
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@dee").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@new-1"));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

// A close the model refuses keeps the selected tab, selected.
#[gpui::test]
fn a_refused_close_of_the_selected_tab_keeps_it_selected(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let model = setup(&mut app, {
            let mut names = Names::new(&["ada", "bob", "cy"]);
            names.keep = vec!["bob"];
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let bob = visual
            .debug_bounds("TAB-name@bob")
            .expect("bob's tab draws")
            .center();
        visual.simulate_click(bob, gpui::Modifiers::default());
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@bob"));

        let click = trace.mark();
        click_close(&mut visual, window, &mut app, "name@bob");
        assert_eq!(
            app.update(|cx| model.read(cx).names.len()),
            3,
            "the model kept bob"
        );
        assert!(visual.debug_bounds("TAB-name@bob").is_some());
        assert_eq!(
            selected(&mut app, "tabs").as_deref(),
            Some("name@bob"),
            "the refused close keeps the selection"
        );
        assert!(trace.held(click, "item-close", "key=bob", &["items unchanged"]));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

// Two item tabs over models of the same type, each its owner's: each
// container's buttons ask its own model, and only its tabs follow.
#[gpui::test]
fn two_item_tabs_over_models_of_the_same_type_ask_their_own_model(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let models = std::rc::Rc::new(std::cell::RefCell::new(BTreeMap::new()));
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let created = models.clone();
            host.provide::<Names>(cx, "tour.owner", move |node, cx| {
                let model = cx.new(|_| Names::new(&["ada", "bob"]));
                created.borrow_mut().insert(node.to_string(), model.clone());
                model
            });
            host.register(cx, "tour.frame", &["children"], |_, slots, _, cx| {
                cx.new(|_| Frame(slots)).into()
            });
            host.register(cx, "tour.owner", &["children"], |_, slots, _, cx| {
                cx.new(|_| Frame(slots)).into()
            });
            host.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
            host.place_root(cx, "root", "tour.frame");
            for side in ["left", "right"] {
                let owner = format!("{side}_owner");
                host.place(cx, &owner, "tour.owner", "root", "children");
                host.place(cx, side, "riso.tabs", &owner, "children");
                host.item_tabs::<Names>(cx, side);
                host.place(cx, &format!("{side}_name"), "tour.card", side, "children");
            }
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let left = models.borrow()["left_owner"].clone();
        let right = models.borrow()["right_owner"].clone();

        let click = trace.mark();
        click_new(&mut visual, window, &mut app, "left");
        assert_eq!(app.update(|cx| left.read(cx).added), 1);
        assert_eq!(app.update(|cx| right.read(cx).added), 0);
        assert!(visual.debug_bounds("TAB-left_name@new-1").is_some());
        assert!(visual.debug_bounds("TAB-right_name@new-1").is_none());
        assert_eq!(
            selected(&mut app, "left").as_deref(),
            Some("left_name@new-1")
        );
        assert_eq!(
            selected(&mut app, "right").as_deref(),
            Some("right_name@ada")
        );
        assert!(trace.nested(click, "item-new", "tabs=left", "items-changed"));
        assert!(trace.held(
            click,
            "item-new",
            "tabs=left",
            &["tab selected", "left_name@new-1"]
        ));

        let click = trace.mark();
        click_close(&mut visual, window, &mut app, "right_name@bob");
        assert_eq!(app.update(|cx| right.read(cx).names.len()), 1);
        assert_eq!(app.update(|cx| left.read(cx).names.len()), 3);
        assert!(visual.debug_bounds("TAB-right_name@bob").is_none());
        assert!(visual.debug_bounds("TAB-left_name@bob").is_some());
        assert!(trace.nested(click, "item-close", "tabs=right", "items-changed"));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

// A new item at the end of a bar that overflows is selected and scrolled
// into view, as feature 25 scrolls a newly selected tab.
#[gpui::test]
fn the_new_items_tab_is_scrolled_into_view(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let names = (1..=20)
            .map(|index| format!("item-{index:02}"))
            .collect::<Vec<_>>();
        let names = names.iter().map(String::as_str).collect::<Vec<_>>();
        setup(&mut app, Names::new(&names));
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let button = visual
            .debug_bounds("NEW-tabs")
            .expect("the new button draws");
        let last = visual
            .debug_bounds("TAB-name@item-20")
            .expect("the last tab draws");
        assert!(
            last.right() > button.origin.x,
            "the bar overflows: {last:?} vs {button:?}"
        );

        let click = trace.mark();
        click_new(&mut visual, window, &mut app, "tabs");
        draw(window, &mut app);
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@new-1"));
        let tab = visual
            .debug_bounds("TAB-name@new-1")
            .expect("the new tab draws");
        let bar = app.update(|cx| shell::node_bounds(cx, "tabs")).unwrap();
        assert!(
            tab.origin.x >= bar.left() && tab.right() <= button.origin.x + px(1.),
            "the new tab is in view: {tab:?}, the button at {button:?}"
        );
        assert!(trace.held(
            click,
            "item-new",
            "tabs=tabs",
            &["tab selected", "name@new-1"]
        ));
    });
}

// Closing the last item leaves the bar with no tab and its new button,
// which still adds an item.
#[gpui::test]
fn closing_the_last_item_leaves_the_new_button(cx: &mut TestAppContext) {
    captured(cx, |cx, _| {
        let mut app = cx.new_app();
        let model = setup(&mut app, Names::new(&["bob"]));
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click_close(&mut visual, window, &mut app, "name@bob");
        assert!(app.update(|cx| model.read(cx).names.is_empty()));
        assert!(visual.debug_bounds("TAB-name@bob").is_none());
        assert_eq!(selected(&mut app, "tabs"), None);
        assert!(
            visual.debug_bounds("NEW-tabs").is_some(),
            "the new button stays"
        );
        click_new(&mut visual, window, &mut app, "tabs");
        assert!(visual.debug_bounds("TAB-name@new-1").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@new-1"));
        assert_eq!(app.update(|cx| shell::user_layer_len(cx)), 0);
    });
}

// Several new clicks before the model changes the items make one pending
// selection: the change that adds both keys selects the first, once, and
// the next addition is not selected.
#[gpui::test]
fn new_clicks_before_the_change_select_the_first_added_key_once(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let model = setup(&mut app, {
            let mut names = Names::new(&["ada", "bob"]);
            names.later = true;
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click_new(&mut visual, window, &mut app, "tabs");
        click_new(&mut visual, window, &mut app, "tabs");
        assert_eq!(app.update(|cx| model.read(cx).added), 2);
        let change = trace.mark();
        app.update(|cx| model.update(cx, |names, cx| names.apply(cx)));
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@new-2").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@new-1"));
        assert_eq!(
            trace.count(change, &["tab selected"]),
            1,
            "one selection for both clicks"
        );
        add_name(&mut app, &model, "dee");
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@dee").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@new-1"));
    });
}

// The next change consumes the pending selection even when it only
// removes: a later addition does not take the selection.
#[gpui::test]
fn a_removal_only_change_consumes_the_pending_selection(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let model = setup(&mut app, {
            let mut names = Names::new(&["ada", "bob", "cy"]);
            names.later = true;
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click_new(&mut visual, window, &mut app, "tabs");
        // A notify that changes nothing does not consume it.
        app.update(|cx| model.update(cx, |_, cx| cx.notify()));
        draw(window, &mut app);
        let removal = trace.mark();
        app.update(|cx| {
            model.update(cx, |names, cx| {
                names.names.retain(|name| name.as_str() != "cy");
                cx.notify();
            })
        });
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@cy").is_none());
        assert!(trace.held(
            removal,
            "items-changed",
            "removed=cy",
            &["DEBUG", "no new item to select"]
        ));
        let addition = trace.mark();
        add_name(&mut app, &model, "dee");
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@dee").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@ada"));
        assert_eq!(trace.count(addition, &["tab selected"]), 0);
    });
}

// A reset of the layout clears a pending selection.
#[gpui::test]
fn a_reset_clears_the_pending_selection(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, model) = setup_with_host(&mut app, {
            let mut names = Names::new(&["ada", "bob"]);
            names.later = true;
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click_new(&mut visual, window, &mut app, "tabs");
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        draw(window, &mut app);
        let addition = trace.mark();
        add_name(&mut app, &model, "dee");
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@dee").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@ada"));
        assert_eq!(trace.count(addition, &["tab selected"]), 0);
    });
}

// A container that leaves the tree loses its pending selection: added
// again, its items' first change selects nothing.
#[gpui::test]
fn a_removed_container_loses_the_pending_selection(cx: &mut TestAppContext) {
    captured(cx, |cx, trace| {
        let mut app = cx.new_app();
        let (host, model) = setup_with_host(&mut app, {
            let mut names = Names::new(&["ada", "bob"]);
            names.later = true;
            names
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        click_new(&mut visual, window, &mut app, "tabs");
        app.update(|cx| host.remove(cx, "tabs"));
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@ada").is_none());
        let back = trace.mark();
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                host.add(window, cx, "tabs", "riso.tabs", "root", "children");
                host.add(window, cx, "name", "tour.card", "tabs", "children");
            })
            .unwrap();
        draw(window, &mut app);
        assert!(
            visual.debug_bounds("TAB-name@ada").is_some(),
            "placed again"
        );
        add_name(&mut app, &model, "dee");
        draw(window, &mut app);
        assert!(visual.debug_bounds("TAB-name@dee").is_some());
        assert_eq!(selected(&mut app, "tabs").as_deref(), Some("name@ada"));
        assert_eq!(trace.count(back, &["tab selected"]), 0);
    });
}
