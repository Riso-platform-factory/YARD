use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use riso_shell::{Hide, SelectNextTab, SelectPreviousTab, ToggleZoom};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

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
        if !event.metadata().target().starts_with("riso_shell")
            && !event.metadata().target().starts_with("riso_layout")
            && event.metadata().target() != "layout_action_diagnostic"
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
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: "layout_action_diagnostic", "{}", record.args());
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

fn draw(window: gpui::WindowHandle<riso_shell::Shell>, cx: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn click(
    visual: &mut gpui::VisualTestContext,
    selector: &'static str,
) -> gpui::Bounds<gpui::Pixels> {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
    bounds
}

/// The innermost group enclosing `line`, or "" when no group is open.
fn group_of(line: &str) -> &str {
    line.split_once(": ")
        .and_then(|(stack, _)| stack.rsplit(" > ").next())
        .unwrap_or("")
}

/// Every group enclosing `line`, outermost first, joined by " > ".
fn stack_of(line: &str) -> &str {
    line.split_once(": ").map(|(stack, _)| stack).unwrap_or("")
}

/// `true` when `line` is an event inside `layout-action` with `node` as its node field.
fn in_action_group(line: &str, node: &str) -> bool {
    let group = group_of(line);
    group.starts_with("INFO layout-action action=") && group.ends_with(&format!(" node={node}"))
}

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn each_action_logs_done_or_refused_inside_its_layout_action_group(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let ext = cx.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            ext.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_root(cx, "root", "riso.row");
            for node in ["a", "b"] {
                ext.place(cx, node, "tour.card", "root", "children");
            }
            ext.place(cx, "one", "riso.tabs", "root", "children");
            ext.place(cx, "s", "tour.card", "one", "children");
            ext.place(cx, "right", "riso.tabs", "root", "children");
            for node in ["t1", "t2"] {
                ext.place(cx, node, "tour.card", "right", "children");
            }
            ext.place(cx, "empty", "riso.tabs", "root", "children");
            for node in ["e1", "e2"] {
                ext.place(cx, node, "tour.card", "empty", "children");
            }
            ext
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // No node has the focus: every action is refused at the shell's root.
        visual.dispatch_action(ToggleZoom);
        visual.dispatch_action(Hide);
        visual.dispatch_action(SelectNextTab);
        visual.dispatch_action(SelectPreviousTab);
        draw(window, cx);
        click(&mut visual, "CARD-b");
        draw(window, cx);
        // `b` sits in a row: no tabs container holds it.
        visual.dispatch_action(SelectNextTab);
        visual.dispatch_action(SelectPreviousTab);
        draw(window, cx);
        visual.dispatch_action(ToggleZoom);
        draw(window, cx);
        visual.dispatch_action(ToggleZoom);
        draw(window, cx);
        visual.dispatch_action(Hide);
        draw(window, cx);
        click(&mut visual, "CARD-t1");
        draw(window, cx);
        visual.dispatch_action(SelectNextTab);
        draw(window, cx);
        // The focus is now on `right`, which selects among its own tabs.
        visual.dispatch_action(SelectPreviousTab);
        draw(window, cx);
        let s_card = click(&mut visual, "CARD-s");
        draw(window, cx);
        visual.dispatch_action(SelectNextTab);
        draw(window, cx);
        // The right of the bar is outside the one tab: it focuses `one`.
        visual.simulate_click(
            gpui::point(
                s_card.right() - gpui::px(4.),
                s_card.origin.y - gpui::px(1.),
            ),
            gpui::Modifiers::default(),
        );
        draw(window, cx);
        visual.dispatch_action(SelectPreviousTab);
        draw(window, cx);
        // Hiding every tab leaves `empty` with no shown tab; its bar still takes the focus.
        cx.update(|cx| {
            ext.set_hidden(cx, "e1", true);
            ext.set_hidden(cx, "e2", true);
        });
        draw(window, cx);
        click(&mut visual, "TABBAR-empty");
        draw(window, cx);
        visual.dispatch_action(SelectNextTab);
        draw(window, cx);
        // Hiding `a` leaves no node focused; the next action is refused at the shell's root.
        click(&mut visual, "CARD-a");
        draw(window, cx);
        visual.dispatch_action(Hide);
        visual.dispatch_action(ToggleZoom);
        draw(window, cx);

        // Zooming and hiding the root are refused inside their own groups.
        let mut app2 = cx.new_app();
        app2.update(riso_ui::init);
        app2.update(|cx| {
            let ext = riso_shell::Extension::new(cx, "tour").unwrap();
            ext.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.place_root(cx, "root", "tour.card");
        });
        let window = app2.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::new_shell)
                .unwrap()
        });
        app2.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        draw(window, &mut app2);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app2);
        click(&mut visual, "CARD-root");
        draw(window, &mut app2);
        visual.dispatch_action(ToggleZoom);
        draw(window, &mut app2);
        visual.dispatch_action(Hide);
        draw(window, &mut app2);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    // An applied action logs `done` at DEBUG inside its `layout-action` group.
    for (node, expected) in [("a", 1), ("b", 3), ("t1", 1), ("right", 1)] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    in_action_group(line.as_str(), node) && line.ends_with(": DEBUG message=done")
                })
                .count(),
            expected,
            "applied actions on {node} in {lines:#?}"
        );
    }
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.ends_with(": DEBUG message=done"))
            .count(),
        6,
        "every done in {lines:#?}"
    );
    // A refused action logs `refused` at INFO inside `layout-action`, with its reason.
    for (node, reason, expected) in [
        ("none", "no node has the focus", 5),
        ("b", "no tabs container holds it", 2),
        ("s", "one tab", 1),
        ("one", "one tab", 1),
        ("empty", "no tabs", 1),
    ] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    in_action_group(line.as_str(), node)
                        && line.contains(": INFO message=refused")
                        && line.contains(reason)
                })
                .count(),
            expected,
            "{reason} on {node} in {lines:#?}"
        );
    }
    assert_eq!(
        lines.iter().filter(|line| line.contains("refused")).count(),
        10,
        "every refused in {lines:#?}"
    );
    // The shell's own operations run inside `layout-action` in their own groups,
    // and the root's refusals are the operations' own.
    for (group, event) in [
        (
            "INFO zoom node=b extension=riso",
            "DEBUG message=zoomed node=b",
        ),
        (
            "INFO zoom-out extension=riso",
            "DEBUG message=zoomed out node=b",
        ),
        (
            "INFO layout-op op=hide node=b extension=riso",
            "DEBUG message=node hidden node=b",
        ),
        (
            "INFO zoom node=root extension=riso",
            "INFO message=the root already fills the window",
        ),
        (
            "INFO layout-op op=hide node=root extension=riso",
            "INFO message=the root cannot be hidden or shown node=root",
        ),
    ] {
        assert!(
            lines.iter().any(|line| {
                stack_of(line.as_str()).contains("layout-action")
                    && group_of(line.as_str()) == group
                    && line.ends_with(&format!(": {event}"))
            }),
            "missing `{group}: {event}` in {lines:#?}"
        );
    }
    // Selecting a tab runs inside the action's group as well.
    for tab in ["t2", "t1"] {
        assert!(
            lines.iter().any(|line| {
                stack_of(line.as_str()).contains("layout-action")
                    && line.contains(": DEBUG message=tab selected")
                    && line.contains(&format!("tabs=right tab={tab}"))
            }),
            "selecting {tab} inside layout-action in {lines:#?}"
        );
    }
    assert!(
        !lines.iter().any(|line| line.contains(": ERROR")),
        "unexpected error in {lines:#?}"
    );
}
