#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The target of the bridged diagnostic log records.
const PROBE: &str = "sizes_as_props_probe";

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
        let target = attributes.metadata().target();
        if target.starts_with("riso_shell") || target.starts_with("riso_paths") {
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
        if (!target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && !target.starts_with("riso_paths")
            && !target.starts_with("riso_ui")
            && target != PROBE)
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

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            _ => tracing::info!(target: PROBE, "{}", record.args()),
        }
    }
    fn flush(&self) {}
}

// A card fills the zone its split gives it and is found as `CARD-<node>`.
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

fn ops_so_far(captured: &Captured) -> usize {
    captured
        .span_stacks
        .iter()
        .filter(|stack| {
            stack
                .last()
                .is_some_and(|span| span.starts_with("layout-op"))
        })
        .count()
}

// Alone in this binary: installs the process-wide diagnostic logger and
// captures every callsite that reaches the traced lines.
#[gpui::test]
fn a_drag_logs_one_props_operation(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            host.place_root(cx, "root", "riso.row");
            for name in ["a", "b", "c"] {
                host.place(cx, name, "tour.card", "root", "children");
            }
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        window
            .update(&mut app, |_, window, _| window.refresh())
            .unwrap();
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| {
                let _ = window.draw(cx);
            })
            .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        // The handle's 4px hitbox is centered on the child's edge.
        let at = {
            let bounds = visual.debug_bounds("CARD-a").expect("CARD-a draws");
            gpui::point(bounds.right(), bounds.center().y)
        };
        // A click on the handle without a move ends the resize and runs no
        // operation.
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
        {
            let captured = trace.0.lock().unwrap();
            assert_eq!(ops_so_far(&captured), 0, "{:#?}", captured.span_stacks);
            assert!(
                captured
                    .lines
                    .iter()
                    .any(|line| line.contains("DEBUG") && line.contains("resize ended")),
                "no `resize ended` line in {:#?}",
                captured.lines
            );
        }
        // The steps of one drag join into one layout-op at the mouse up.
        let target = gpui::point(px(450.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            gpui::point(px(375.), at.y),
            gpui::MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(ops_so_far(&trace.0.lock().unwrap()), 1);
        // A double click on the handle is one more props operation.
        visual.simulate_mouse_move(target, None, Default::default());
        visual.simulate_event(gpui::MouseDownEvent {
            position: target,
            button: gpui::MouseButton::Left,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(ops_so_far(&trace.0.lock().unwrap()), 2);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let stacks: Vec<Vec<String>> = captured
        .span_stacks
        .iter()
        .map(|stack| stack.iter().map(|span| span.replace('"', "")).collect())
        .collect();
    let op = "layout-op op=props node=root extension=riso";
    // The drag and the double click each ran the same group once.
    let ops: Vec<&Vec<String>> = stacks
        .iter()
        .filter(|stack| stack.last().is_some_and(|span| span.as_str() == op))
        .collect();
    assert_eq!(ops.len(), 2, "{stacks:#?}");
    // Inside the group: the model's `props set` names the `shares` key, and the
    // split reads its props in its `props-read` group.
    let sets: Vec<&String> = lines
        .iter()
        .filter(|line| {
            line.starts_with(op)
                && line.contains("DEBUG")
                && line.contains("message=props set")
                && line.contains("keys=shares")
        })
        .collect();
    assert_eq!(sets.len(), 2, "{lines:#?}");
    let reads: Vec<&Vec<String>> = stacks
        .iter()
        .filter(|stack| {
            stack
                .windows(2)
                .any(|pair| pair[0] == op && pair[1] == "props-read node=root component=riso.row")
        })
        .collect();
    assert_eq!(reads.len(), 2, "{stacks:#?}");
    // Each is recorded in the user layer like the other operations.
    let recorded: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with(op) && line.contains("DEBUG message=recorded"))
        .collect();
    assert_eq!(recorded.len(), 2, "{lines:#?}");
}
