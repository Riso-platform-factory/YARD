use gpui::{AppContext, IntoElement, ParentElement, Styled, TestAppContext, px};
use riso_shell as shell;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

// The one test in this binary installs the process-wide diagnostic logger.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

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

fn errors() -> Vec<String> {
    RECORDS
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message.clone())
        .collect()
}

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
        // `context` is asked at TRACE on every frame; this file keeps it.
        let target = event.metadata().target();
        if !(target.starts_with("riso_shell") || target.starts_with("riso_layout")) {
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

// The model a `tour.counter` node gives to the nodes under it: a number.
struct Count(u32);

type Shown = Rc<RefCell<Vec<(String, Option<u32>)>>>;

// A view that asks for a Count on every frame and records what it receives.
struct Asking {
    node: String,
    shown: Shown,
}

impl gpui::Render for Asking {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let value = shell::context::<Count>(cx, &self.node).map(|model| model.read(cx).0);
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div().size_full().child(
            gpui::canvas(
                move |_, _, _| shown.borrow_mut().push((node, value)),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }
}

// A view that draws its `body` slot.
struct Container {
    node: String,
    slots: shell::Slots,
    shown: Shown,
}

impl gpui::Render for Container {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let shown = self.shown.clone();
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::canvas(
                    move |_, _, _| shown.borrow_mut().push((node, None)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(
                self.slots
                    .views("body")
                    .into_iter()
                    .map(|view| gpui::div().flex_1().min_h_0().child(view)),
            )
    }
}

fn draw(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext, shown: &Shown) {
    shown.borrow_mut().clear();
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn open_and_draw(app: &mut TestAppContext, shown: &Shown) -> gpui::WindowHandle<shell::Shell> {
    let window = app.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
    window.update(app, |_, window, _| window.refresh()).unwrap();
    draw(window, app, shown);
    window
}

// True when a `context {node, model, from}` trace line was recorded, with
// either quoting of the field values.
fn has_context_line(lines: &[String], node: &str, model: &str, from: &str) -> bool {
    lines.iter().any(|line| {
        line.contains("TRACE")
            && line.contains("message=context")
            && [("node", node), ("model", model), ("from", from)]
                .iter()
                .all(|(name, value)| {
                    line.contains(&format!("{name}={value}"))
                        || line.contains(&format!("{name}=\"{value}\""))
                })
    })
}

// Alone in this binary: every call reaching captured lines uses this subscriber.
#[gpui::test]
fn contexts_log_declarations_creation_asks_and_the_refused_move(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let shown = Shown::default();
        let host = app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let counter_shown = shown.clone();
            host.register(cx, "tour.counter", &["body"], move |node, slots, _, cx| {
                cx.new(|_| Container {
                    node: node.to_string(),
                    slots,
                    shown: counter_shown.clone(),
                })
                .into()
            });
            host.provide::<Count>(cx, "tour.counter", |_, cx| cx.new(|_| Count(5)));
            let card_shown = shown.clone();
            host.register(cx, "tour.count_card", &[], move |node, _, _, cx| {
                cx.new(|_| Asking {
                    node: node.to_string(),
                    shown: card_shown.clone(),
                })
                .into()
            });
            host.require_context::<Count>(cx, "tour.count_card");
            let leaf_shown = shown.clone();
            host.register(cx, "tour.leaf", &[], move |node, _, _, cx| {
                cx.new(|_| Asking {
                    node: node.to_string(),
                    shown: leaf_shown.clone(),
                })
                .into()
            });
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "counter", "tour.counter", "root", "children");
            host.place(cx, "left", "tour.count_card", "counter", "body");
            host.place(cx, "outside", "tour.leaf", "root", "children");
            host
        });
        let window = open_and_draw(&mut app, &shown);
        // A refused move logs its INFO inside layout-op{op=move}: left requires
        // a model that nothing above root gives and the window does not.
        app.update(|cx| host.move_node(cx, "left", "root", "children", 0));
        // The window is given a model: outside asks for it on the next frame.
        app.update(|cx| {
            let model = cx.new(|_| Count(7));
            host.provide_window::<Count>(cx, model);
        });
        draw(window, &mut app, &shown);
        // A refused declaration logs an ERROR in its group.
        app.update(|cx| host.provide::<Count>(cx, "tour.counter", |_, cx| cx.new(|_| Count(0))));
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    // Each declaration opens context-declare{component, model, kind} and logs
    // `declared` inside it.
    for (component, model, kind) in [
        (Some("tour.counter"), "Count", "provide"),
        (Some("tour.count_card"), "Count", "require"),
        (None, "Count", "window"),
    ] {
        let declared = captured.spans.values().any(|label| {
            label.starts_with("context-declare")
                && label.contains(model)
                && label.contains(kind)
                && component.is_none_or(|component| label.contains(component))
                && captured
                    .lines
                    .contains(&format!("{label}: DEBUG message=declared"))
        });
        assert!(
            declared,
            "missing context-declare for {component:?}/{model}/{kind} with `declared` in spans {:?} lines {:#?}",
            captured.spans.values().collect::<Vec<_>>(),
            captured.lines
        );
    }
    // `context created {node, model}` inside `component-create`.
    assert!(
        captured.lines.iter().any(|line| {
            line.starts_with("component-create node=counter")
                && line.contains("DEBUG")
                && line.contains("context created")
                && line.contains("counter")
                && line.contains("Count")
        }),
        "missing `context created` inside component-create in {:#?}",
        captured.lines
    );
    // The refused move's INFO inside layout-op{op=move}.
    assert!(
        captured.lines.iter().any(|line| {
            line.starts_with(r#"layout-op op="move" node="left" extension="tour": INFO"#)
                && line.contains("left requires a Count model, and none is above root")
        }),
        "missing the refused move's INFO in {:#?}",
        captured.lines
    );
    // Views ask on every frame: from the giving node, none, then the window.
    for (node, from) in [
        ("left", "counter"),
        ("outside", "none"),
        ("outside", "window"),
    ] {
        assert!(
            has_context_line(&captured.lines, node, "Count", from),
            "missing `context` line for {node} from {from} in {:#?}",
            captured.lines
        );
    }
    drop(captured);
    // The refused second provide of Count for tour.counter is an ERROR.
    let errors = errors();
    assert!(
        errors
            .iter()
            .any(|message| message.contains("tour.counter")),
        "missing the refused provide ERROR in {errors:?}"
    );
}
