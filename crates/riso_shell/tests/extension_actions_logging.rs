#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

gpui::actions!(test_ext, [Increment, Ping]);

/// The target of the test's own lines: what the extension's code logs, and the
/// bridged diagnostic log.
const PROBE: &str = "extension_actions_probe";

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
            && event.metadata().target() != PROBE
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        // Every enclosing group, outermost first.
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
            tracing::error!(target: PROBE, "{}", record.args());
        }
    }
    fn flush(&self) {}
}

struct Card {
    node: String,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut card = gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full();
        // A component handles its own action on the element that tracks the
        // node's handle; what it logs is the extension's, not the shell's.
        if let Some(handle) = riso_shell::dev::focus_handle(cx, &self.node) {
            card = card.track_focus(&handle).key_context("test_ext").on_action(
                |_: &Increment, _, _| {
                    tracing::info!(target: PROBE, "increment");
                },
            );
        }
        card
    }
}

fn draw(window: gpui::WindowHandle<riso_shell::dev::Shell>, cx: &mut TestAppContext) {
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

/// Every group enclosing `line`, outermost first, joined by " > ".
fn stack_of(line: &str) -> &str {
    line.split_once(": ").map(|(stack, _)| stack).unwrap_or("")
}

/// The level and fields of `line`, without the enclosing groups.
fn event_of(line: &str) -> &str {
    line.split_once(": ")
        .map(|(_, event)| event)
        .unwrap_or(line)
}

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn binding_and_handling_actions_log_their_groups_lines_and_refusals(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    cx.update(riso_ui::init);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            // The startup wraps each extension's setup in `extension-init`.
            let _group = tracing::info_span!("extension-init", extension = "test_ext").entered();
            let ext = riso_shell::dev::Extension::new(cx, "test_ext").unwrap();
            ext.register(cx, "test_ext.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            ext.bind_keys(
                cx,
                &[
                    ("ctrl-alt-i", Box::new(Increment), "test_ext"),
                    ("ctrl-alt-", Box::new(Ping), "Shell"),
                    ("ctrl-alt-p", Box::new(Ping), "Shell"),
                ],
            );
            ext.on_action::<Ping>(cx, |_, _, _| {
                tracing::info!(target: PROBE, "ping");
            });
            ext.on_action::<riso_shell::dev::Hide>(cx, |_, _, _| {
                panic!("test_ext cannot handle riso::Hide")
            });
            ext.place_root(cx, "root", "riso.row");
            for node in ["a", "b"] {
                ext.place(cx, node, "test_ext.card", "root", "children");
            }
        });
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "CARD-a");
        draw(window, cx);
        // The component's own action runs on the focused card; the shell does not see it.
        visual.simulate_keystrokes("ctrl-alt-i");
        draw(window, cx);
        // The window-wide action runs wherever the focus is, inside its own group.
        visual.simulate_keystrokes("ctrl-alt-p");
        draw(window, cx);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let spans: Vec<String> = captured
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .collect();
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let init = "INFO extension-init extension=test_ext";
    // `bind_keys` opens `keys-bind{extension, count}` inside `extension-init`.
    let keys_bind = {
        let groups: Vec<&String> = spans
            .iter()
            .filter(|span| {
                span.starts_with("INFO keys-bind ")
                    && span.contains("extension=test_ext")
                    && span.contains("count=3")
            })
            .collect();
        assert_eq!(
            groups.len(),
            1,
            "one `keys-bind` group for `test_ext` in {spans:#?}"
        );
        groups[0].clone()
    };
    // Each bound binding gets one DEBUG line with its keys, action, and context.
    let bound_stack = format!("{init} > {keys_bind}");
    for (keys, action, context) in [
        ("ctrl-alt-i", "Increment", "test_ext"),
        ("ctrl-alt-p", "Ping", "Shell"),
    ] {
        assert_eq!(
            lines
                .iter()
                .filter(|line| {
                    stack_of(line) == bound_stack
                        && event_of(line).starts_with("DEBUG")
                        && event_of(line).contains(keys)
                        && event_of(line).contains(action)
                        && event_of(line).contains(context)
                })
                .count(),
            1,
            "`{keys_bind}` names {keys} for {action} in {context} once in {lines:#?}"
        );
    }
    // The unreadable binding is refused inside `keys-bind`, with its keys and
    // the reason; the others are bound.
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                group_of(line) == keys_bind
                    && event_of(line).starts_with("ERROR")
                    && event_of(line).contains("ctrl-alt-")
            })
            .count(),
        1,
        "the unreadable `ctrl-alt-` errors inside `{keys_bind}` in {lines:#?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| group_of(line) == keys_bind)
            .count(),
        3,
        "one line per binding inside `{keys_bind}` in {lines:#?}"
    );
    // Registering a window-wide handler logs `action handled` in `extension-init`.
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                group_of(line) == init
                    && event_of(line).starts_with("DEBUG")
                    && event_of(line).contains("action handled")
                    && event_of(line).contains("test_ext")
                    && event_of(line).contains("Ping")
            })
            .count(),
        1,
        "`action handled` inside `{init}` in {lines:#?}"
    );
    // Handling another namespace's action is refused; `riso::Hide` stays the
    // shell's.
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                event_of(line).starts_with("ERROR")
                    && line.contains("extension test_ext cannot handle riso::Hide")
            })
            .count(),
        1,
        "`extension test_ext cannot handle riso::Hide` in {lines:#?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| event_of(line).starts_with("ERROR"))
            .count(),
        2,
        "exactly the unreadable binding and the foreign action error in {lines:#?}"
    );
    // A window-wide action runs inside `extension-action{extension, action}`.
    let extension_action = {
        let groups: Vec<&String> = spans
            .iter()
            .filter(|span| {
                span.starts_with("INFO extension-action ")
                    && span.contains("extension=test_ext")
                    && span.contains("Ping")
            })
            .collect();
        assert_eq!(
            groups.len(),
            1,
            "one `extension-action` group for `Ping` in {spans:#?}"
        );
        groups[0].clone()
    };
    // The shell sees only the window-wide action: no `extension-action` group
    // names the component's `Increment`.
    assert_eq!(
        spans
            .iter()
            .filter(|span| span.starts_with("INFO extension-action"))
            .count(),
        1,
        "only the window-wide run opens `extension-action` in {spans:#?}"
    );
    // The handler's own lines are inside the group.
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                group_of(line) == extension_action && event_of(line) == "INFO message=ping"
            })
            .count(),
        1,
        "`ping` inside `{extension_action}` in {lines:#?}"
    );
    // The component's action is logged by the extension's own code, inside no
    // group of the shell's.
    assert_eq!(
        lines
            .iter()
            .filter(|line| event_of(line) == "INFO message=increment")
            .count(),
        1,
        "the card logs `increment` in {lines:#?}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| {
                group_of(line).starts_with("INFO extension-action")
                    && event_of(line).starts_with("INFO")
            })
            .count(),
        1,
        "nothing but `ping` runs inside `extension-action` in {lines:#?}"
    );
}
