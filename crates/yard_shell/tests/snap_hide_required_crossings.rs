use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use serde_json::json;
use std::sync::{Arc, Mutex};
use yard_shell as shell;

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

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
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", event.metadata().level(), fields.0));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
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

// Capture locally in this isolated test binary, including across redraws.
fn lines_since(trace: &Trace, mark: usize) -> Vec<String> {
    trace.0.lock().unwrap()[mark..]
        .iter()
        .map(|line| line.replace('"', ""))
        .collect()
}

#[gpui::test]
fn required_refusals_log_once_per_threshold_entry_and_gesture(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        app.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                })
                .into()
            });
            host.place_root_with_props(cx, "root", "yard.row", json!({"sizes": {"side": 240}}));
            host.place(cx, "side", "tour.card", "root", "children");
            host.place(cx, "main", "tour.card", "root", "children");
            host.require(cx, "tour.card");
        });
        let window = open_and_draw(&mut app);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &app);
        let at = handle(&mut visual, "CARD-side");
        let start = trace.0.lock().unwrap().len();
        let count = || {
            lines_since(&trace, start)
                .iter()
                .filter(|line| line.contains("required child kept"))
                .count()
        };
        for gesture in 0..2 {
            visual.simulate_mouse_move(at, None, Default::default());
            visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
            for x in [10., 15., 20.] {
                visual.simulate_mouse_move(
                    gpui::point(px(x), at.y),
                    gpui::MouseButton::Left,
                    Default::default(),
                );
                draw(window, &mut app);
            }
            assert_eq!(
                count(),
                gesture * 2 + 1,
                "one refusal while below threshold"
            );
            visual.simulate_mouse_move(
                gpui::point(px(100.), at.y),
                gpui::MouseButton::Left,
                Default::default(),
            );
            for x in [10., 15.] {
                visual.simulate_mouse_move(
                    gpui::point(px(x), at.y),
                    gpui::MouseButton::Left,
                    Default::default(),
                );
            }
            assert_eq!(count(), gesture * 2 + 2, "one more on reentry");
            visual.simulate_mouse_move(at, gpui::MouseButton::Left, Default::default());
            visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
            draw(window, &mut app);
        }
        let lines = lines_since(&trace, start);
        let kept: Vec<_> = lines
            .iter()
            .filter(|line| line.contains("required child kept"))
            .collect();
        assert!(
            kept.iter().all(|line| line.contains("DEBUG")
                && line.contains("split=root")
                && line.contains("child=side")),
            "{kept:?}"
        );
    });
}
