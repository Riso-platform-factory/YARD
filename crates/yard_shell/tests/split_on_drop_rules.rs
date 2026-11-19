use gpui::{AppContext, IntoElement, ParentElement, Styled, TestAppContext, px};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use yard_shell as shell;

// Every test reaches the same tracing callsites, so captures run under one lock.
static LOCK: Mutex<()> = Mutex::new(());

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
        if !event.metadata().target().starts_with("yard_shell")
            && !event.metadata().target().starts_with("yard_layout")
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

type Cards = Rc<RefCell<BTreeMap<String, gpui::Entity<Card>>>>;

struct Card {
    count: usize,
    slots: shell::Slots,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full().children(self.slots.views("body"))
    }
}

struct SavedLayout(std::path::PathBuf);

impl SavedLayout {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "yard-splitdrop-rules-{}-{nanos}.json",
            std::process::id()
        )))
    }
}

impl Drop for SavedLayout {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Project;

fn setup(cx: &mut TestAppContext) -> (shell::Extension, Cards) {
    let cards = Cards::default();
    let created = cards.clone();
    let extension = cx.update(|cx| {
        yard_ui::init(cx);
        let extension = shell::Extension::new(cx, "rules").unwrap();
        for component in ["rules.card", "rules.provider", "rules.needs", "rules.frame"] {
            let created = created.clone();
            extension.register(cx, component, &["body"], move |node, slots, _, cx| {
                let card = cx.new(|_| Card { count: 0, slots });
                created.borrow_mut().insert(node.to_string(), card.clone());
                card.into()
            });
        }
        extension.provide::<Project>(cx, "rules.provider", |_, cx| cx.new(|_| Project));
        extension.require_context::<Project>(cx, "rules.needs");
        extension.slot_for_one(cx, "rules.frame", "body");
        extension.place_root(cx, "root", "yard.row");
        extension.place(cx, "provider", "rules.provider", "root", "children");
        extension.place(cx, "left", "yard.tabs", "provider", "body");
        extension.place(cx, "a", "rules.card", "left", "children");
        extension.place(cx, "b", "rules.needs", "left", "children");
        extension.place(cx, "right", "yard.tabs", "root", "children");
        extension.place(cx, "c", "rules.card", "right", "children");
        extension.place(cx, "d", "rules.card", "right", "children");
        extension.place(cx, "frame", "rules.frame", "root", "children");
        extension.place(cx, "inside", "yard.tabs", "frame", "body");
        extension.place(cx, "e", "rules.card", "inside", "children");
        extension
    });
    (extension, cards)
}

fn draw(window: gpui::WindowHandle<shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn drag(
    window: gpui::WindowHandle<shell::Shell>,
    cx: &mut TestAppContext,
    from: &'static str,
    target: &str,
    zone: &str,
) {
    let bounds = cx.update(|cx| shell::node_bounds(cx, target)).unwrap();
    let end = match zone {
        "top" => gpui::point(bounds.center().x, bounds.top() + px(8.)),
        "right" => gpui::point(bounds.right() - px(8.), bounds.center().y),
        "bottom" => gpui::point(bounds.center().x, bounds.bottom() - px(8.)),
        _ => bounds.center(),
    };
    let mut visual = gpui::VisualTestContext::from_window(window.into(), &*cx);
    let start = visual.debug_bounds(from).unwrap().center();
    visual.simulate_mouse_move(start, None, Default::default());
    visual.simulate_mouse_down(start, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        gpui::point(start.x + px(5.), start.y),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_move(end, gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    if zone == "center" && from.strip_prefix("TAB-") == Some(target) {
        assert!(visual.debug_bounds("DROP-OVERLAY-center").is_some());
    }
    visual.simulate_mouse_up(end, gpui::MouseButton::Left, Default::default());
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
    for zone in [
        "DROP-OVERLAY-left",
        "DROP-OVERLAY-right",
        "DROP-OVERLAY-top",
        "DROP-OVERLAY-bottom",
        "DROP-OVERLAY-center",
    ] {
        assert!(
            visual.debug_bounds(zone).is_none(),
            "overlay remains: {zone}"
        );
    }
}

#[gpui::test]
fn refusals_preserve_prior_layer_file_selection_and_views(cx: &mut TestAppContext) {
    let _guard = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let (extension, cards) = setup(cx);
        let saved = SavedLayout::new();
        let path = &saved.0;
        cx.update(|cx| shell::set_layout_file(cx, path.clone()));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(1200.), px(800.)));
        draw(window, cx);
        // Preserve an existing recorded layer and a selected requiring tab.
        cx.update(|cx| {
            extension.set_hidden(cx, "d", true);
            cards.borrow()["b"].update(cx, |card, _| card.count = 7);
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &*cx);
        let tab = visual.debug_bounds("TAB-b").unwrap().center();
        visual.simulate_click(tab, Default::default());
        draw(window, cx);
        let before = std::fs::read(path).unwrap();
        let b = cards.borrow()["b"].clone();
        let source = cx.update(|cx| shell::node_bounds(cx, "b")).unwrap();
        let target = cx.update(|cx| shell::node_bounds(cx, "c")).unwrap();
        // The fourth operation lacks the model supplied only by provider.
        drag(window, cx, "TAB-b", "c", "top");
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(
            cx.update(|cx| shell::selected_tab(cx, "left"))
                .unwrap()
                .as_str(),
            "b"
        );
        assert_eq!(cx.update(|cx| shell::node_bounds(cx, "b")).unwrap(), source);
        assert_eq!(cx.update(|cx| shell::node_bounds(cx, "c")).unwrap(), target);
        assert_eq!(cx.update(|cx| b.read(cx).count), 7);
        assert_eq!(cards.borrow()["b"].entity_id(), b.entity_id());
        assert!(cx.update(|cx| shell::node_bounds(cx, "split-1")).is_none());
        assert_undone(&trace, 3);
        trace.0.lock().unwrap().lines.clear();
        // The very first add is refused in frame's slot for one.
        drag(window, cx, "TAB-a", "e", "bottom");
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(
            cx.update(|cx| shell::selected_tab(cx, "inside"))
                .unwrap()
                .as_str(),
            "e"
        );
        assert_undone(&trace, 0);
        // Dropping the selected tab on itself is a cycle even though wrapping
        // its tabs container would otherwise leave a legal final tree.
        drag(window, cx, "TAB-b", "b", "top");
        drag(window, cx, "TAB-b", "b", "center");
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
        assert_eq!(std::fs::read(path).unwrap(), before);
    });
}

#[gpui::test]
fn required_target_splits_with_equal_shares_despite_preferred_size(cx: &mut TestAppContext) {
    let _guard = LOCK.lock().unwrap();
    let (extension, _) = setup(cx);
    cx.update(|cx| {
        extension.require(cx, "rules.card");
        extension.size_hints(
            cx,
            "rules.card",
            shell::SizeHints {
                min: gpui::size(gpui::rems(1.), gpui::rems(1.)),
                preferred: gpui::size(None, Some(gpui::rems(8.))),
            },
        );
        extension.place(cx, "plain", "rules.card", "root", "children");
        extension.place(cx, "inner", "yard.column", "a", "body");
        extension.place(cx, "pad", "rules.card", "inner", "children");
    });
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(1600.), px(900.)));
    draw(window, cx);
    // An inner split-child center blocks an outer tabs-center drop.
    drag(window, cx, "TAB-b", "pad", "center");
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 0);
    assert_eq!(
        cx.update(|cx| shell::selected_tab(cx, "left"))
            .unwrap()
            .as_str(),
        "a"
    );
    drag(window, cx, "TAB-a", "plain", "bottom");
    let split = cx.update(|cx| shell::node_bounds(cx, "split-1")).unwrap();
    let plain = cx.update(|cx| shell::node_bounds(cx, "plain")).unwrap();
    let tabs = cx.update(|cx| shell::node_bounds(cx, "tabs-1")).unwrap();
    assert!((f32::from(plain.size.height - split.size.height / 2.)).abs() <= 1.);
    assert!((f32::from(tabs.size.height - split.size.height / 2.)).abs() <= 1.);
    assert!(tabs.center().y > plain.center().y);
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 4);
}

fn assert_undone(trace: &Trace, done: usize) {
    let captured = trace.0.lock().unwrap();
    let undone: Vec<_> = captured
        .lines
        .iter()
        .filter(|line| line.contains("split on drop undone"))
        .collect();
    assert_eq!(undone.len(), 1, "{undone:?}");
    assert!(
        undone[0].contains(&format!("DEBUG message=split on drop undone done={done}")),
        "{}",
        undone[0]
    );
}

#[gpui::test]
fn center_context_refusal_records_nothing(cx: &mut TestAppContext) {
    let _guard = LOCK.lock().unwrap();
    let (extension, _) = setup(cx);
    let saved = SavedLayout::new();
    cx.update(|cx| {
        shell::set_layout_file(cx, saved.0.clone());
    });
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(px(1200.), px(800.)));
    draw(window, cx);
    cx.update(|cx| extension.set_hidden(cx, "d", true));
    draw(window, cx);
    let before = std::fs::read(&saved.0).unwrap();
    let bounds =
        ["a", "c", "left", "right"].map(|node| cx.update(|cx| shell::node_bounds(cx, node)));
    drag(window, cx, "TAB-b", "c", "center");
    assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
    assert_eq!(std::fs::read(&saved.0).unwrap(), before);
    assert_eq!(
        cx.update(|cx| shell::selected_tab(cx, "left"))
            .unwrap()
            .as_str(),
        "a"
    );
    assert_eq!(
        cx.update(|cx| shell::selected_tab(cx, "right"))
            .unwrap()
            .as_str(),
        "c"
    );
    assert_eq!(
        bounds,
        ["a", "c", "left", "right"].map(|node| cx.update(|cx| shell::node_bounds(cx, node)))
    );
}

#[gpui::test]
fn tabs_on_own_child_edge_is_refused_before_operations(cx: &mut TestAppContext) {
    let _guard = LOCK.lock().unwrap();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let (extension, _) = setup(cx);
        cx.update(|cx| {
            extension.place(cx, "nested", "yard.tabs", "right", "children");
            extension.place(cx, "leaf", "rules.card", "nested", "children");
        });
        let saved = SavedLayout::new();
        cx.update(|cx| shell::set_layout_file(cx, saved.0.clone()));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(px(1200.), px(800.)));
        draw(window, cx);
        cx.update(|cx| extension.set_hidden(cx, "d", true));
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &*cx);
        let tab = visual.debug_bounds("TAB-nested").unwrap().center();
        visual.simulate_click(tab, Default::default());
        draw(window, cx);
        let before = std::fs::read(&saved.0).unwrap();
        let nodes = [
            "root", "provider", "left", "a", "right", "nested", "leaf", "frame", "inside", "e",
        ];
        let bounds = nodes.map(|node| cx.update(|cx| shell::node_bounds(cx, node)));
        trace.0.lock().unwrap().lines.clear();
        let mark = trace.0.lock().unwrap().spans.len();
        drag(window, cx, "TAB-nested", "leaf", "top");
        assert_eq!(cx.update(|cx| shell::user_layer_len(cx)), 1);
        assert_eq!(std::fs::read(&saved.0).unwrap(), before);
        assert_eq!(
            bounds,
            nodes.map(|node| cx.update(|cx| shell::node_bounds(cx, node)))
        );
        assert_eq!(
            cx.update(|cx| shell::selected_tab(cx, "right"))
                .unwrap()
                .as_str(),
            "nested"
        );
        assert_eq!(
            cx.update(|cx| shell::selected_tab(cx, "nested"))
                .unwrap()
                .as_str(),
            "leaf"
        );
        let captured = trace.0.lock().unwrap();
        let infos: Vec<_> = captured
            .lines
            .iter()
            .filter(|line| line.contains(": INFO"))
            .collect();
        assert_eq!(infos.len(), 1, "{infos:?}");
        assert!(
            infos[0].contains("moving a node onto itself would make a cycle"),
            "{infos:?}"
        );
        assert!(
            !captured.spans.iter().any(|(id, label)| *id > mark as u64
                && (label.starts_with("layout-op") || label.starts_with("split-on-drop"))),
            "{:?}",
            captured.spans
        );
    });
}
