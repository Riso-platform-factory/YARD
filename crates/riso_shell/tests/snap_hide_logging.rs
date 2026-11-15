#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The target of the bridged diagnostic log records.
const PROBE: &str = "snap_hide_probe";

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
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
            && !target.starts_with("riso_paths")
            && !target.starts_with("riso_ui")
            && target != PROBE
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

// A record in the diagnostic log is bridged into the trace, to see it inside
// the lifecycle group it belongs to.
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

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
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
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app);
    window
}

// The handle's 4px hitbox is centered on the child's edge, so the edge itself
// is inside it.
fn handle(visual: &mut gpui::VisualTestContext, before: &'static str) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(before).expect("the child draws");
    gpui::point(bounds.right(), bounds.center().y)
}

// The collapsed line of a hidden first child sits at the split's left edge,
// between the edge and the next child: the next child's leading edge is
// inside its hitbox.
fn collapsed_line(
    visual: &mut gpui::VisualTestContext,
    next: &'static str,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visual.debug_bounds(next).expect("the next child draws");
    gpui::point(bounds.left(), bounds.center().y)
}

// The lines and spans captured since a mark, with quotes stripped.
fn lines_since(trace: &Trace, mark: usize) -> Vec<String> {
    trace.0.lock().unwrap().lines[mark..]
        .iter()
        .map(|line| line.replace('"', ""))
        .collect()
}

fn stacks_since(trace: &Trace, mark: usize) -> Vec<Vec<String>> {
    trace.0.lock().unwrap().span_stacks[mark..]
        .iter()
        .map(|stack| stack.iter().map(|span| span.replace('"', "")).collect())
        .collect()
}

fn mark(trace: &Trace) -> (usize, usize) {
    let captured = trace.0.lock().unwrap();
    (captured.lines.len(), captured.span_stacks.len())
}

// A snap shut gives one `layout-op{op=hide, extension=riso}` and a snap open
// one `layout-op{op=show, extension=riso}` with the set-props of the resize;
// each time a child crosses half its minimum in a drag, `snap preview` logs
// once; a required child is kept, never snapped.
#[gpui::test]
fn snaps_log_their_operations_and_one_preview_per_crossing(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let host = app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            host.place_root_with_props(cx, "root", "riso.row", json!({"sizes": {"side": 240}}));
            host.place(cx, "side", "tour.card", "root", "children");
            host.place(cx, "main", "tour.card", "root", "children");
            host
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);

        // Dragging there and back before releasing: one `snap preview` each
        // time `side` crosses half its minimum, and no `hide`.
        let (lines_mark, stacks_mark) = mark(&trace);
        let at = handle(&mut visual, "CARD-side");
        let shut = gpui::point(px(30.), at.y);
        let back = gpui::point(px(200.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(shut, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(back, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(back, gpui::MouseButton::Left, Default::default());
        {
            let lines = lines_since(&trace, lines_mark);
            let previews: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("DEBUG") && line.contains("snap preview"))
                .collect();
            assert_eq!(previews.len(), 2, "one preview per crossing: {lines:#?}");
            assert!(previews[0].contains("shut=true"), "{}", previews[0]);
            assert!(previews[1].contains("shut=false"), "{}", previews[1]);
            let stacks = stacks_since(&trace, stacks_mark);
            assert!(
                !stacks.iter().flatten().any(|span| {
                    span.starts_with("layout-op op=hide") && span.contains("node=side")
                }),
                "a snap taken back records no hide: {stacks:#?}"
            );
        }

        // A snap shut: one more preview, and `layout-op{op=hide, node=side,
        // extension=riso}` at the mouse up.
        let (lines_mark, stacks_mark) = mark(&trace);
        let at = handle(&mut visual, "CARD-side");
        let target = gpui::point(px(30.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        {
            let lines = lines_since(&trace, lines_mark);
            let previews: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("DEBUG") && line.contains("snap preview"))
                .collect();
            assert_eq!(
                previews.len(),
                1,
                "one preview for the one crossing: {lines:#?}"
            );
            assert!(previews[0].contains("shut=true"), "{}", previews[0]);
            let stacks = stacks_since(&trace, stacks_mark);
            let hides = stacks
                .iter()
                .filter(|stack| {
                    stack
                        .last()
                        .is_some_and(|span| span == "layout-op op=hide node=side extension=riso")
                })
                .count();
            assert_eq!(hides, 1, "one hide operation: {stacks:#?}");
        }

        // A snap open on the collapsed line: one preview the other way, and
        // `layout-op{op=show, node=side, extension=riso}` at the mouse up,
        // with the set-props that records the size.
        draw(window, &mut app);
        let (lines_mark, stacks_mark) = mark(&trace);
        let at = collapsed_line(&mut visual, "CARD-main");
        let target = gpui::point(px(300.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        {
            let lines = lines_since(&trace, lines_mark);
            let previews: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("DEBUG") && line.contains("snap preview"))
                .collect();
            assert_eq!(
                previews.len(),
                1,
                "one preview for the one crossing: {lines:#?}"
            );
            assert!(previews[0].contains("shut=false"), "{}", previews[0]);
            let stacks = stacks_since(&trace, stacks_mark);
            let shows = stacks
                .iter()
                .filter(|stack| {
                    stack
                        .last()
                        .is_some_and(|span| span == "layout-op op=show node=side extension=riso")
                })
                .count();
            assert_eq!(shows, 1, "one show operation: {stacks:#?}");
            assert!(
                stacks.iter().flatten().any(|span| {
                    span.starts_with("layout-op op=props")
                        && span.contains("node=root")
                        && span.contains("extension=riso")
                }),
                "the size dragged to is recorded with a set-props: {stacks:#?}"
            );
        }

        // A required child is never snapped: dragging its handle past half
        // its minimum logs `required child kept` and no `hide` runs.
        app.update(|cx| host.require(cx, "tour.card"));
        let (lines_mark, stacks_mark) = mark(&trace);
        let at = handle(&mut visual, "CARD-side");
        let target = gpui::point(px(10.), at.y);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        {
            let lines = lines_since(&trace, lines_mark);
            let kept: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("DEBUG") && line.contains("required child kept"))
                .collect();
            assert!(!kept.is_empty(), "{lines:#?}");
            assert!(kept[0].contains("side"), "{}", kept[0]);
            let stacks = stacks_since(&trace, stacks_mark);
            assert!(
                !stacks.iter().flatten().any(|span| {
                    span.starts_with("layout-op op=hide") && span.contains("node=side")
                }),
                "a required child is never snapped shut: {stacks:#?}"
            );
        }

        let captured = trace.0.lock().unwrap();
        assert!(captured.entered.is_empty());
        let errors: Vec<&String> = captured
            .lines
            .iter()
            .filter(|line| line.contains("ERROR"))
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    });
}
