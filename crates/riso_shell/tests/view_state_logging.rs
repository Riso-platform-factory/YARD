use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_layout::NodeId;
use riso_shell as shell;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const PROBE: &str = "view_state_probe";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-shell-view-state-logging-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// The diagnostic logger: this file's one test installs it once, and every
// `log` line the shell writes is recorded and repeated into the trace so
// both logs can be read in one timeline.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
            || metadata.target().starts_with("riso_state")
            || metadata.target().starts_with("riso_paths")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        RECORDS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((record.level(), record.args().to_string()));
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            log::Level::Info => tracing::info!(target: PROBE, "{}", record.args()),
            log::Level::Debug => tracing::debug!(target: PROBE, "{}", record.args()),
            log::Level::Trace => tracing::trace!(target: PROBE, "{}", record.args()),
        }
    }
    fn flush(&self) {}
}

fn records() -> Vec<(log::Level, String)> {
    RECORDS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn warnings() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, message)| message)
        .collect()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, message)| message)
        .collect()
}

// The groups the code opens and the events it writes: `spans` names each
// group, `entered` is the stack of open groups, and each event is one `line`
// plus the stack that was open when it was written.
#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, String>,
    entered: Vec<u64>,
    lines: Vec<String>,
    event_stacks: Vec<Vec<String>>,
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
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        if !(target.starts_with("riso_shell")
            || target.starts_with("riso_state")
            || target.starts_with("riso_paths")
            || target == PROBE)
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let group = captured
            .entered
            .last()
            .map(|id| captured.spans[id].clone())
            .unwrap_or_else(|| "none".to_string());
        let stack = captured
            .entered
            .iter()
            .map(|id| captured.spans[id].clone())
            .collect();
        captured.event_stacks.push(stack);
        captured
            .lines
            .push(format!("{group}: {}{}", event.metadata().level(), fields.0));
    }
    fn enter(&self, id: &tracing::span::Id) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .push(id.into_u64());
    }
    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
                .pop(),
            Some(id.into_u64())
        );
    }
}

impl Trace {
    fn mark(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines
            .len()
    }
    // The lines since `mark`, without the quoting Debug fields add.
    fn since(&self, mark: usize) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .lines[mark..]
            .iter()
            .map(|line| line.replace('"', ""))
            .collect()
    }
    // `(entered groups, line)` for each event since `mark`, the same way.
    fn events_since(&self, mark: usize) -> Vec<(Vec<String>, String)> {
        let captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        captured.event_stacks[mark..]
            .iter()
            .map(|stack| {
                stack
                    .iter()
                    .map(|label| label.replace('"', ""))
                    .collect::<Vec<_>>()
            })
            .zip(
                captured.lines[mark..]
                    .iter()
                    .map(|line| line.replace('"', "")),
            )
            .collect()
    }
}

// A card fills the zone its container gives it and is found as `CARD-<node>`.
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

// `host` registers `tour.card` and places a row of the card `a` and the
// `right` tabs container over `b`, `c`, `d`.
fn extension(app: &mut TestAppContext) -> shell::Extension {
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
        host.place(cx, "a", "tour.card", "root", "children");
        host.place(cx, "right", "riso.tabs", "root", "children");
        for node in ["b", "c", "d"] {
            host.place(cx, node, "tour.card", "right", "children");
        }
        host
    })
}

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

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

// The contract's logging, alone in this binary: `state-register` for
// `riso.tabs` and `riso.window` when the shell starts; the `state given` of
// feature 1 inside `component-create`; `tab given {tabs, tab}` at `debug`
// once, at the container's first layout; `zoom given {node}` at `debug`
// inside `shell-create`; and a `WARN` for a state that cannot be read while
// the default is kept.
#[gpui::test]
fn view_state_logging_covers_the_contract_rows(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    let temp = TempDir::new();
    let file = temp.path().join("state.json");
    tracing::subscriber::with_default(trace.clone(), || {
        // A saved tab `d` and a saved zoom `a` are given back when the
        // window opens.
        fs::write(
            &file,
            json!({
                "version": 1,
                "windows": [{
                    "right": {"component": "riso.tabs", "state": {"selected": "d"}},
                    "window": {"component": "riso.window", "state": {"zoomed": "a"}},
                }],
            })
            .to_string(),
        )
        .unwrap();
        let mut app = cx.new_app();
        let _host = extension(&mut app);
        app.update(|cx| shell::set_state_file(cx, file.clone()));
        let mark = trace.mark();
        let window = open_and_draw(&mut app);
        let lines = trace.since(mark);
        let events = trace.events_since(mark);

        // The shell registers its two states when it starts.
        for component in ["riso.tabs", "riso.window"] {
            assert!(
                lines.iter().any(|line| {
                    line.starts_with(&format!("state-register component={component}:"))
                        && line.contains("DEBUG")
                        && line.contains("registered")
                }),
                "missing `registered` inside state-register for {component}: {lines:#?}"
            );
        }

        // The saved tab is given to the container as its state, inside
        // `component-create` — the `state given {node}` of feature 1.
        assert!(
            events.iter().any(|(stack, line)| {
                line.contains("state given")
                    && line.contains("node=right")
                    && stack
                        .iter()
                        .any(|label| label.starts_with("component-create"))
            }),
            "no `state given {{node}}` for `right` inside component-create: {events:#?}"
        );

        // And it is applied once, at the container's first layout.
        let tab_given = lines
            .iter()
            .filter(|line| line.contains("tab given"))
            .count();
        assert_eq!(tab_given, 1, "`tab given` is not logged once: {lines:#?}");
        assert!(
            lines.iter().any(|line| {
                line.contains("DEBUG")
                    && line.contains("tab given")
                    && line.contains("tabs=right")
                    && line.contains("tab=d")
            }),
            "bad `tab given {{tabs, tab}}`: {lines:#?}"
        );

        // The saved zoom is given inside `shell-create`.
        assert!(
            events.iter().any(|(stack, line)| {
                line.contains("DEBUG")
                    && line.contains("zoom given")
                    && line.contains("node=a")
                    && stack.iter().any(|label| label.starts_with("shell-create"))
            }),
            "no `zoom given {{node}}` inside shell-create: {events:#?}"
        );
        assert_eq!(
            app.update(|cx| shell::selected_tab(cx, "right")),
            Some(id("d"))
        );
        assert_eq!(app.update(|cx| shell::zoomed(cx)).as_deref(), Some("a"));

        // Frames that lay the container out again do not log `tab given`
        // again.
        draw(window, &mut app);
        draw(window, &mut app);
        let lines = trace.since(mark);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("tab given"))
                .count(),
            1,
            "`tab given` logged more than once: {lines:#?}"
        );

        // A state that cannot be read — `selected` that is not a string or
        // `null`, `window`'s state that is not an object — warns, and the
        // default is kept while the other state still applies.
        for (right, window, expected_tab, expected_zoom) in [
            (
                json!({"selected": 7}),
                json!({"zoomed": "a"}),
                "b",
                Some("a"),
            ),
            (json!({"selected": "d"}), json!("oops"), "d", None),
        ] {
            let bad = TempDir::new();
            let file = bad.path().join("state.json");
            fs::write(
                &file,
                json!({
                    "version": 1,
                    "windows": [{
                        "right": {"component": "riso.tabs", "state": right},
                        "window": {"component": "riso.window", "state": window},
                    }],
                })
                .to_string(),
            )
            .unwrap();
            let mut broken = cx.new_app();
            let _host = extension(&mut broken);
            broken.update(|cx| shell::set_state_file(cx, file.clone()));
            let warnings_before = warnings().len();
            let _window = open_and_draw(&mut broken);
            assert!(
                warnings().len() > warnings_before,
                "an unreadable view state did not warn: {right} / {window}"
            );
            assert_eq!(
                broken.update(|cx| shell::selected_tab(cx, "right")),
                Some(id(expected_tab))
            );
            assert_eq!(
                broken.update(|cx| shell::zoomed(cx)).as_deref(),
                expected_zoom
            );
        }
        assert!(errors().is_empty(), "{:?}", errors());
    });
    assert!(
        trace
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entered
            .is_empty()
    );
}
