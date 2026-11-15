#![cfg(feature = "dev")]

use gpui::{IntoElement, TestAppContext, px};
use riso_shell::dev as shell;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

// The target of the bridged diagnostic log records.
const PROBE: &str = "props_probe";

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

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct CardProps {
    title: String,
    tint: String,
    count: u32,
}

impl CardProps {
    fn new(title: &str, tint: &str, count: u32) -> Self {
        Self {
            title: title.into(),
            tint: tint.into(),
            count,
        }
    }
}

type Told = Rc<RefCell<Vec<(String, CardProps)>>>;

struct Card {
    node: String,
    told: Told,
}

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

impl shell::PropsView<CardProps> for Card {
    fn set_props(&mut self, props: CardProps, _: &mut gpui::Context<Self>) {
        self.told.borrow_mut().push((self.node.clone(), props));
    }
}

fn position(lines: &[String], what: &str, predicate: impl Fn(&str) -> bool) -> usize {
    lines
        .iter()
        .position(|line| predicate(line))
        .unwrap_or_else(|| panic!("missing {what} in {lines:#?}"))
}

// Alone in this binary: installs the process-wide diagnostic logger and
// captures every callsite that reaches the traced lines.
#[gpui::test]
fn set_props_logs_its_group_the_read_and_the_telling(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    let told = Told::default();
    let before_reset = tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let host = app.update(|cx| {
            riso_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            let told = told.clone();
            host.register_with_props::<CardProps, Card>(
                cx,
                "tour.card",
                &["body"],
                move |_, node, _, _, _| Card {
                    node: node.to_string(),
                    told: told.clone(),
                },
            );
            host.place_root(cx, "root", "riso.row");
            host.place_with_props(
                cx,
                "alpha",
                "tour.card",
                "root",
                "children",
                json!({"title": "Alpha", "tint": "accent"}),
            );
            host.place_with_props(
                cx,
                "broken",
                "tour.card",
                "root",
                "children",
                json!({"title": "Broken", "tint": 42}),
            );
            // Props that do not encode as a JSON object are an ERROR in the group
            // of the call.
            host.place_with_props(cx, "badplace", "tour.card", "root", "children", vec![1]);
            host
        });
        let window = app.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
                .unwrap()
        });
        app.simulate_window_resize(window.into(), gpui::size(px(900.), px(600.)));
        app.update(|cx| {
            host.set_props(cx, "alpha", json!({"tint": "warning", "count": 2}));
            host.set_props(cx, "ghost", json!({"title": "G"}));
            host.set_props(cx, "alpha", json!("not an object"));
        });
        let before_reset = trace.0.lock().unwrap().lines.len();
        gpui::AnyWindowHandle::from(window)
            .update(&mut app, |_, window, cx| host.reset_layout(window, cx))
            .unwrap();
        // The view was told the merged props, then its defaults after the reset.
        assert_eq!(
            *told.borrow(),
            [
                ("alpha".to_string(), CardProps::new("Alpha", "warning", 2)),
                ("alpha".to_string(), CardProps::new("Alpha", "accent", 0)),
            ]
        );
        before_reset
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
    let op = "layout-op op=props node=alpha extension=tour";

    // The props-read group sits inside the operation's group.
    assert!(
        stacks.iter().any(|stack| stack
            .windows(2)
            .any(|pair| pair[0] == op && pair[1] == "props-read node=alpha component=tour.card")),
        "no props-read group inside the op in {stacks:#?}"
    );
    // Inside the op: `props set`, then the read, then `view told`.
    let set = position(&lines, "props set", |line| {
        line.starts_with(op) && line.contains("DEBUG message=props set")
    });
    assert!(lines[set].contains("tint") && lines[set].contains("count"));
    let read = position(&lines[set + 1..], "props read", |line| {
        line.starts_with("props-read node=alpha component=tour.card")
            && line.contains("DEBUG message=props read")
    }) + set
        + 1;
    let told_line = position(&lines[read + 1..], "view told", |line| {
        line.starts_with(op)
            && line.contains("DEBUG")
            && line.contains("view told")
            && line.contains("alpha")
    }) + read
        + 1;
    assert!(set < read && read < told_line);
    // It is recorded like the other operations.
    assert!(
        lines
            .iter()
            .any(|line| { line.starts_with(op) && line.contains("DEBUG message=recorded") })
    );
    // A node not in the tree is refused inside the op, at INFO.
    assert!(lines.iter().any(|line| {
        line.starts_with("layout-op op=props node=ghost extension=tour")
            && line.contains("INFO")
            && line.contains("the node is not in the tree")
    }));
    // Props that do not encode as an object are an ERROR in the group of the call.
    assert!(
        lines
            .iter()
            .any(|line| { line.starts_with(op) && line.contains(": ERROR") })
    );
    assert!(
        lines.iter().any(|line| {
            line.starts_with("node-place node=badplace") && line.contains(": ERROR")
        })
    );
    // A bad value logs one WARN, inside the props-read group.
    let warns: Vec<&String> = lines[..before_reset]
        .iter()
        .filter(|line| line.contains(": WARN"))
        .collect();
    assert_eq!(warns.len(), 1, "{lines:#?}");
    assert!(
        warns[0].starts_with(
            "props-read node=broken component=tour.card: WARN message=prop tint of node broken cannot be read:"
        ),
        "{}",
        warns[0]
    );
    assert!(warns[0].ends_with("the default is used"), "{}", warns[0]);
    // The reset tells the view in its own group.
    assert!(lines[before_reset..].iter().any(|line| {
        line.starts_with("layout-reset extension=tour:")
            && line.contains("DEBUG")
            && line.contains("view told")
            && line.contains("alpha")
    }));
}
