#![cfg(feature = "dev")]

use gpui::{AppContext, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Once},
};

// Alone in this binary: it shares one process-wide diagnostic logger and one
// subscriber for the lifecycle trace, so no other test reaches the same
// lines.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static INIT: Once = Once::new();

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
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
    static LOCK: Mutex<()> = Mutex::new(());
    INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    LOCK.lock().unwrap()
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
        // The UI kit's lines are kept at every level: the `trace` `rendering
        // icon button` of `riso_ui::IconButton` is the only line that shows a
        // toggle's drawn `disabled` flag.
        if !(target.starts_with("riso_shell") || target.starts_with("riso_ui"))
            || (target.starts_with("riso_shell")
                && *event.metadata().level() == tracing::Level::TRACE)
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
    fn mark(&self) -> (usize, usize) {
        let captured = self.0.lock().unwrap();
        (captured.lines.len(), captured.span_stacks.len())
    }
    fn stacks_since(&self, mark: usize) -> Vec<Vec<String>> {
        self.0.lock().unwrap().span_stacks[mark..].to_vec()
    }
    fn icon_buttons(&self, mark: usize) -> Vec<String> {
        self.0.lock().unwrap().lines[mark..]
            .iter()
            .filter(|line| line.contains("rendering icon button"))
            .cloned()
            .collect()
    }
}

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            let _ = window.draw(cx);
        })
        .unwrap();
}

// A toggle's node bounds are the bounds of its button: a click there is a
// click on the button.
fn click_toggle(visual: &mut gpui::VisualTestContext, toggle: &str) {
    let bounds = visual
        .update(|_, cx| shell::node_bounds(cx, toggle))
        .unwrap_or_else(|| panic!("toggle {toggle} is drawn"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

// The index of the stack where `group` holds `op` directly, if any: the two
// labels are adjacent entries, `group` first, anywhere in the stack — the
// click may sit inside outer groups. Quotes are stripped on both sides: a
// `&str` field renders `node="x"` and a `%`-recorded `NodeId` renders
// `node=x`; the contract names the fields, not their quoting.
fn stack_index(stacks: &[Vec<String>], group: &str, op: &str) -> Option<usize> {
    let unquote = |label: &str| label.replace('"', "");
    stacks.iter().position(|stack| {
        stack
            .windows(2)
            .any(|pair| unquote(&pair[0]) == unquote(group) && unquote(&pair[1]) == unquote(op))
    })
}

// A click on a toggle opens one `toggle{toggle, node, shown}` group holding
// the `layout-op` groups of its operations — a `hide` and a `show` in that
// order with `alone` — and `shown` is what the node's shown state becomes.
#[gpui::test]
fn a_toggle_click_groups_its_operations_and_a_disabled_toggle_warns_once(cx: &mut TestAppContext) {
    let _guard = lock();
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |_, _, _, cx| cx.new(|_| Card).into());
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "side", "riso.column", "root", "children");
            host.place(cx, "left", "tour.card", "side", "children");
            host.place(cx, "extra", "tour.card", "side", "children");
            host.place(cx, "main", "tour.card", "root", "children");
            host.place_with_props(
                cx,
                "toggle_left",
                "riso.toggle",
                "status_bar",
                "left",
                json!({"node": "left", "icon": "info", "alone": true}),
            );
            host.place_with_props(
                cx,
                "toggle_extra",
                "riso.toggle",
                "status_bar",
                "left",
                json!({"node": "extra", "icon": "plus", "alone": true}),
            );
            host.place_with_props(
                cx,
                "toggle_ghost",
                "riso.toggle",
                "status_bar",
                "right",
                json!({"node": "ghost", "icon": "warning"}),
            );
            host.place_with_props(
                cx,
                "toggle_self",
                "riso.toggle",
                "status_bar",
                "right",
                json!({"node": "toggle_self", "icon": "info"}),
            );
            host.set_hidden(cx, "extra", true);
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        window
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        // `open_window`, the resize, and `refresh` each drew a frame already:
        // only this one's `rendering icon button` lines are counted.
        let (lines_mark, _) = trace.mark();
        draw(window, &mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        // The toggles of a node that is not in the tree and of itself are
        // drawn disabled, and each warns once in the diagnostic log.
        let buttons = trace.icon_buttons(lines_mark);
        assert_eq!(
            buttons
                .iter()
                .filter(|line| line.contains("disabled=true"))
                .count(),
            2,
            "{buttons:?}"
        );
        assert_eq!(
            buttons
                .iter()
                .filter(|line| line.contains("disabled=false"))
                .count(),
            2,
            "{buttons:?}"
        );
        let warns = warnings();
        assert_eq!(warns.len(), 2, "{warns:?}");
        // The WARN names the bad toggle or its target; either way it names one.
        for (toggle, target) in [("toggle_ghost", "ghost"), ("toggle_self", "toggle_self")] {
            assert!(
                warns
                    .iter()
                    .any(|warn| warn.contains(toggle) || warn.contains(target)),
                "missing a WARN for {toggle} in {warns:?}"
            );
        }

        // `extra` is hidden and `left` is shown: showing `extra` with `alone`
        // hides `left` first — a `hide` and then a `show`, held by one
        // `toggle` group whose `shown` is what `extra`'s becomes.
        let (_, mark) = trace.mark();
        click_toggle(&mut visual, "toggle_extra");
        let stacks = trace.stacks_since(mark);
        let group = r#"toggle toggle="toggle_extra" node="extra" shown=true"#;
        let hide = stack_index(
            &stacks,
            group,
            r#"layout-op op="hide" node="left" extension="riso""#,
        );
        let show = stack_index(
            &stacks,
            group,
            r#"layout-op op="show" node="extra" extension="riso""#,
        );
        assert!(
            hide.is_some_and(|hide| show.is_some_and(|show| hide < show)),
            "the hide of left comes before the show of extra inside {group}: {stacks:#?}"
        );

        // Clicking the now-shown node hides it: `shown` is what it becomes.
        let (_, mark) = trace.mark();
        click_toggle(&mut visual, "toggle_extra");
        let stacks = trace.stacks_since(mark);
        let group = r#"toggle toggle="toggle_extra" node="extra" shown=false"#;
        assert!(
            stack_index(
                &stacks,
                group,
                r#"layout-op op="hide" node="extra" extension="riso""#
            )
            .is_some(),
            "missing the hide of extra inside {group}: {stacks:#?}"
        );

        // Both nodes of the slot are hidden now, so `alone` hides nothing:
        // showing `left` holds only its `show`.
        let (_, mark) = trace.mark();
        click_toggle(&mut visual, "toggle_left");
        let stacks = trace.stacks_since(mark);
        let group = r#"toggle toggle="toggle_left" node="left" shown=true"#;
        assert!(
            stack_index(
                &stacks,
                group,
                r#"layout-op op="show" node="left" extension="riso""#
            )
            .is_some(),
            "missing the show of left inside {group}: {stacks:#?}"
        );
        assert!(
            !stacks.iter().any(|stack| stack
                .last()
                .is_some_and(|label| label.starts_with(r#"layout-op op="hide""#))),
            "nothing else is shown in the slot: {stacks:#?}"
        );

        // A click on a disabled toggle opens no `toggle` group and runs no
        // `layout-op`.
        let (_, mark) = trace.mark();
        click_toggle(&mut visual, "toggle_ghost");
        click_toggle(&mut visual, "toggle_self");
        let stacks = trace.stacks_since(mark);
        assert!(
            !stacks
                .iter()
                .flatten()
                .any(|label| { label.starts_with("toggle ") || label.starts_with("layout-op") }),
            "a disabled toggle opens no group and runs no operation: {stacks:#?}"
        );

        // Another frame warns nothing more: each WARN was once.
        draw(window, &mut app);
        assert_eq!(warnings().len(), 2, "{:?}", warnings());
        assert!(errors().is_empty(), "{:?}", errors());
    });
    assert!(trace.0.lock().unwrap().entered.is_empty());
}
