use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs.
const PROBE: &str = "empty_nodes_probe";

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
        let target = event.metadata().target();
        if !target.starts_with("riso_shell")
            && !target.starts_with("riso_layout")
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

// The one process-wide diagnostic logger this file installs once; alone here,
// it cannot collide with another test's logger.
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

// A card whose `empty` flag is the state its view reports through `Emptiable`.
struct Card {
    node: String,
    empty: bool,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
    }
}

impl shell::Emptiable for Card {
    fn is_empty(&self, _: &gpui::App) -> bool {
        self.empty
    }
}

// The view each node got, so the test can change its state and notify.
type Views = Rc<RefCell<BTreeMap<String, gpui::WeakEntity<Card>>>>;

// Draw with a forced refresh: every call is a frame.
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

// The view changes its state and notifies.
fn set_empty(app: &mut TestAppContext, views: &Views, node: &str, empty: bool) {
    app.update(|cx| {
        views
            .borrow()
            .get(node)
            .and_then(|view| view.upgrade())
            .expect("the node has a view")
            .update(cx, |card, cx| {
                card.empty = empty;
                cx.notify();
            });
    });
}

fn mark(trace: &Trace) -> usize {
    trace.0.lock().unwrap().lines.len()
}

// The lines captured since a mark, with quotes stripped.
fn lines_since(trace: &Trace, mark: usize) -> Vec<String> {
    trace.0.lock().unwrap().lines[mark..]
        .iter()
        .map(|line| line.replace('"', ""))
        .collect()
}

// Alone in this binary: every call reaching these log lines shares this one
// subscriber, so no callsite caches "nobody listens" out from under it.
#[gpui::test]
fn emptiness_logs_its_register_group_each_read_and_one_line_per_change(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let views = Views::default();
        app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let guest = shell::Extension::new(cx, "guest").unwrap();
            let card_views = views.clone();
            host.register(cx, "tour.card", &[], move |node, _, _, cx| {
                let card = cx.new(|_| Card {
                    node: node.to_string(),
                    empty: false,
                });
                card_views
                    .borrow_mut()
                    .insert(node.to_string(), card.downgrade());
                card.into()
            });
            // tour.card says it can be empty, in its own group.
            host.register_empty::<Card>(cx, "tour.card");
            guest.register(cx, "guest.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    node: node.to_string(),
                    empty: false,
                })
                .into()
            });
            // Refused: a component outside the extension's namespace, and a
            // second declaration of the same component.
            host.register_empty::<Card>(cx, "guest.card");
            host.register_empty::<Card>(cx, "tour.card");
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "b", "tour.card", "root", "children");
        });
        let window = open_and_draw(&mut app);

        // A component that says it can be empty opens `empty-register{component}`
        // and logs `can be empty` at debug; a refusal logs an ERROR in its own
        // group and declares nothing.
        {
            let captured = trace.0.lock().unwrap();
            let lines: Vec<String> = captured
                .lines
                .iter()
                .map(|line| line.replace('"', ""))
                .collect();
            assert!(
                lines.iter().any(
                    |line| line.starts_with("empty-register component=tour.card:")
                        && line.contains("DEBUG")
                        && line.contains("message=can be empty")
                ),
                "the group's `can be empty` at debug: {lines:#?}"
            );
            for component in ["guest.card", "tour.card"] {
                assert!(
                    lines.iter().any(|line| line
                        .starts_with(&format!("empty-register component={component}:"))
                        && line.contains("ERROR")),
                    "the refusal names {component}: {lines:#?}"
                );
            }
            assert!(
                !lines.iter().any(
                    |line| line.starts_with("empty-register component=guest.card:")
                        && line.contains("can be empty")
                ),
                "a refused component declares nothing: {lines:#?}"
            );
        }

        // A node's emptiness is read when its container lays out: a trace
        // `empty {node, empty}` on every frame.
        let before = mark(&trace);
        for _ in 0..3 {
            draw(window, &mut app);
        }
        {
            let lines = lines_since(&trace, before);
            // One `empty` read per frame for each node a container lays
            // out: `a` and `b`, the children of `root`, and no other.
            let reads = |node: &str| {
                lines
                    .iter()
                    .filter(|line| {
                        line.contains("TRACE")
                            && line.contains("message=empty")
                            && line.contains(&format!("node={node}"))
                    })
                    .count()
            };
            assert_eq!(
                reads("a"),
                3,
                "an `empty` trace read for `a` on each of three frames: {lines:#?}"
            );
            assert_eq!(
                reads("b"),
                3,
                "an `empty` trace read for `b` on each of three frames: {lines:#?}"
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.contains("TRACE") && line.contains("message=empty"))
                    .count(),
                6,
                "only `a` and `b` are laid out by a container: {lines:#?}"
            );
            assert!(
                !lines
                    .iter()
                    .any(|line| line.contains("message=empty") && !line.contains("TRACE")),
                "an `empty` read above trace: {lines:#?}"
            );
        }

        // `a` becomes empty: one `emptiness changed` at debug, and a frame
        // without a change logs none.
        let before = mark(&trace);
        set_empty(&mut app, &views, "a", true);
        draw(window, &mut app);
        {
            let lines = lines_since(&trace, before);
            let changes: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("message=emptiness changed"))
                .collect();
            assert_eq!(changes.len(), 1, "once per change: {lines:#?}");
            assert!(changes[0].contains("DEBUG"), "{}", changes[0]);
            assert!(changes[0].contains("node=a"), "{}", changes[0]);
            assert!(changes[0].contains("empty=true"), "{}", changes[0]);
        }
        let before = mark(&trace);
        draw(window, &mut app);
        assert!(
            !lines_since(&trace, before)
                .iter()
                .any(|line| line.contains("message=emptiness changed")),
            "a frame without a change logs none"
        );
        // `a` stops being empty: one more line, the other way.
        let before = mark(&trace);
        set_empty(&mut app, &views, "a", false);
        draw(window, &mut app);
        {
            let lines = lines_since(&trace, before);
            let changes: Vec<&String> = lines
                .iter()
                .filter(|line| line.contains("message=emptiness changed"))
                .collect();
            assert_eq!(changes.len(), 1, "{lines:#?}");
            assert!(changes[0].contains("DEBUG"), "{}", changes[0]);
            assert!(changes[0].contains("node=a"), "{}", changes[0]);
            assert!(changes[0].contains("empty=false"), "{}", changes[0]);
        }

        let captured = trace.0.lock().unwrap();
        assert!(captured.entered.is_empty());
    });
}
