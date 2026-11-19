use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use yard_shell::{FocusDown, FocusLeft, FocusRight, FocusUp};

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
        captured.spans.insert(
            id,
            format!(
                "{} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ),
        );
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_shell")
            && event.metadata().target() != "focus_by_direction_diagnostic"
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        // Every enclosing group, outermost first, so a group inside `layout-action` is seen.
        let group = if captured.entered.is_empty() {
            String::from("none")
        } else {
            captured
                .entered
                .iter()
                .map(|id| captured.spans[id].as_str())
                .collect::<Vec<_>>()
                .join(" > ")
        };
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

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "focus_by_direction_diagnostic", "{}", record.args());
        }
    }
    fn flush(&self) {}
}

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

fn draw(window: gpui::WindowHandle<yard_shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

/// The innermost group enclosing `line`, or "" when no group is open.
fn group_of(line: &str) -> &str {
    line.split_once(": ")
        .and_then(|(stack, _)| stack.rsplit(" > ").next())
        .unwrap_or("")
}

/// `true` when `line` is an event inside `layout-action` for `action` on `node`.
fn in_action_group(line: &str, action: &str, node: &str) -> bool {
    let group = group_of(line);
    group.starts_with("INFO layout-action action=")
        && group.contains(action)
        && group.ends_with(&format!(" node={node}"))
}

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn each_move_logs_done_or_refused_inside_its_layout_action_group(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(yard_ui::init);
    let trace = Trace::default();
    let mut a_bounds = None;
    let mut b_bounds = None;
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            let ext = yard_shell::Extension::new(cx, "tour").unwrap();
            ext.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_root(cx, "root", "yard.row");
            for node in ["a", "b"] {
                ext.place(cx, node, "tour.card", "root", "children");
            }
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // No node has the focus: every move is refused at the shell's root.
        visual.dispatch_action(FocusLeft);
        visual.dispatch_action(FocusRight);
        visual.dispatch_action(FocusUp);
        visual.dispatch_action(FocusDown);
        draw(window, cx);
        click(&mut visual, "CARD-a");
        draw(window, cx);
        a_bounds = visual.update(|_, cx| yard_shell::node_bounds(cx, "a"));
        // `a` fills the left half of the window: nothing is on its left, above, or below.
        visual.dispatch_action(FocusLeft);
        visual.dispatch_action(FocusUp);
        visual.dispatch_action(FocusDown);
        draw(window, cx);
        visual.dispatch_action(FocusRight);
        draw(window, cx);
        b_bounds = visual.update(|_, cx| yard_shell::node_bounds(cx, "b"));
        // `b` fills the right half: nothing is on its right.
        visual.dispatch_action(FocusRight);
        draw(window, cx);
        visual.dispatch_action(FocusLeft);
        draw(window, cx);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let a_bounds = a_bounds.expect("`a` was drawn");
    let b_bounds = b_bounds.expect("`b` was drawn");
    // An applied move logs `done` at DEBUG inside its `layout-action` group, with `target`.
    for (action, node, target) in [("FocusRight", "a", "b"), ("FocusLeft", "b", "a")] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    in_action_group(line.as_str(), action, node)
                        && line.contains(": DEBUG message=done")
                        && line.contains(&format!("target={target}"))
                })
                .count(),
            1,
            "the {action} move on {node} names target={target} in {lines:#?}"
        );
    }
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains(": DEBUG message=done"))
            .count(),
        2,
        "every done in {lines:#?}"
    );
    // With no node focused, each move logs `refused` at INFO, at the shell's root.
    for action in ["FocusLeft", "FocusRight", "FocusUp", "FocusDown"] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    in_action_group(line.as_str(), action, "none")
                        && line.contains(": INFO message=refused")
                        && line.contains("no node has the focus")
                })
                .count(),
            1,
            "{action} refused when no node has the focus in {lines:#?}"
        );
    }
    // With no leaf in the direction, the move logs `refused` at INFO with its
    // reason, the focused node's `bounds`, and the probe `point`.
    for (action, node, reason, bounds) in [
        ("FocusLeft", "a", "no node on the left", a_bounds),
        ("FocusUp", "a", "no node above", a_bounds),
        ("FocusDown", "a", "no node below", a_bounds),
        ("FocusRight", "b", "no node on the right", b_bounds),
    ] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    in_action_group(line.as_str(), action, node)
                        && line.contains(": INFO message=refused")
                        && line.contains(reason)
                        && line.contains(&format!("bounds={bounds:?}"))
                        && line.contains("point=")
                })
                .count(),
            1,
            "{action} on {node}: {reason} with bounds and point in {lines:#?}"
        );
    }
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("message=refused"))
            .count(),
        8,
        "every refused in {lines:#?}"
    );
    // A move of the focus logs `focus-change{node}` like any other, once for the
    // click on `a` and once for the move back to `a`, and once for the move to `b`.
    for (node, expected) in [("a", 2), ("b", 1)] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    group_of(line.as_str()) == format!("INFO focus-change node={node}")
                        && line.ends_with(": DEBUG message=focused")
                })
                .count(),
            expected,
            "focus-change to {node} in {lines:#?}"
        );
    }
    assert!(
        !lines.iter().any(|line| line.contains(": ERROR")),
        "unexpected error in {lines:#?}"
    );
}
