use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use yard_shell as shell;

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs, with the records' groups.
const PROBE: &str = "new_window_probe";

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
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
        if attributes.metadata().target().starts_with("yard_shell") {
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
        if !(target.starts_with("yard_shell")
            || target.starts_with("yard_layout")
            || target == PROBE)
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

impl Trace {
    fn mark(&self) -> usize {
        self.0.lock().unwrap().lines.len()
    }
    fn since(&self, mark: usize) -> Vec<String> {
        self.0.lock().unwrap().lines[mark..].to_vec()
    }
    fn stack_mark(&self) -> usize {
        self.0.lock().unwrap().span_stacks.len()
    }
    fn stacks_since(&self, mark: usize) -> Vec<Vec<String>> {
        self.0.lock().unwrap().span_stacks[mark..].to_vec()
    }
}

// The `name=value` field of a span label or a captured line. A captured line
// puts `:` after the group label, so a trailing colon is dropped first.
fn field<'a>(label: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=");
    label.split(' ').find_map(|part| {
        part.strip_suffix(':')
            .unwrap_or(part)
            .strip_prefix(key.as_str())
    })
}

fn field_is(label: &str, name: &str, value: &str) -> bool {
    field(label, name).is_some_and(|found| found.trim_matches('"') == value)
}

// The id a window's groups name it by, in the encodings a `WindowId` takes:
// the raw or quoted number, or the debug form of the ID.
fn id_variants(id: u64) -> [String; 4] {
    let window = gpui::WindowId::from(id);
    [
        id.to_string(),
        format!("\"{id}\""),
        format!("{window:?}"),
        format!("\"{window:?}\""),
    ]
}

fn window_field_is(label: &str, name: &str, id: u64) -> bool {
    field(label, name).is_some_and(|found| {
        id_variants(id)
            .iter()
            .any(|variant| variant.as_str() == found)
    })
}

// Since `mark`, a span matching `inner` was created inside a `window-new`
// group: `window-new` is an ancestor in the span's creation stack.
fn inside_window_new(trace: &Trace, mark: usize, inner: impl Fn(&str) -> bool) -> bool {
    trace.stacks_since(mark).iter().any(|stack| {
        stack.last().is_some_and(|label| inner(label))
            && stack[..stack.len() - 1]
                .iter()
                .any(|label| label.starts_with("window-new"))
    })
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

// Draw without forcing a refresh: operations must schedule it themselves.
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

fn window_id(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> u64 {
    window
        .update(app, |_, window, _| {
            window.window_handle().window_id().as_u64()
        })
        .unwrap()
}

// `yard::NewWindow` dispatched on `window`.
fn dispatch_new_window(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    app.dispatch_action(window.into(), shell::NewWindow);
}

// `yard::NewWindow` opens a group `window-new`. Without an opener the action
// is refused with one WARN inside it; with an opener the group holds the new
// window's `window{id}` and `shell-create{root, window}`.
#[gpui::test]
fn window_new_warns_without_an_opener_and_holds_the_new_windows_groups(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
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
            host.place_root(cx, "root", "yard.row");
            host.place(cx, "a", "tour.card", "root", "children");
        });
        let first = open_and_draw(&mut app);

        // Without an opener the action is refused with one WARN inside
        // `window-new`, and nothing opens.
        let mark = trace.mark();
        dispatch_new_window(first, &mut app);
        assert_eq!(
            app.update(|cx| cx.windows().len()),
            1,
            "a window opened without an opener"
        );
        let lines = trace.since(mark);
        let warnings: Vec<&String> = lines
            .iter()
            .filter(|line| line.starts_with("window-new") && line.contains(": WARN"))
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "the refusal did not WARN once inside window-new: {lines:#?}"
        );

        // With an opener, `window-new` holds the new window's `window{id}`
        // and `shell-create{root, window}`.
        let opened = Rc::new(RefCell::new(Vec::new()));
        let recorded = opened.clone();
        app.update(move |cx| {
            shell::set_window_opener(cx, move |cx| {
                let window = cx
                    .open_window(gpui::WindowOptions::default(), shell::new_shell)
                    .unwrap();
                recorded.borrow_mut().push(window);
            });
        });
        let mark = trace.stack_mark();
        dispatch_new_window(first, &mut app);
        let second = opened.borrow().first().copied().unwrap();
        let second_id = window_id(second, &mut app);
        assert!(
            inside_window_new(&trace, mark, |label| {
                label.starts_with("window ") && window_field_is(label, "id", second_id)
            }),
            "window-new does not hold the new window's window{{id}}: {:#?}",
            trace.stacks_since(mark)
        );
        assert!(
            inside_window_new(&trace, mark, |label| {
                label.starts_with("shell-create")
                    && field_is(label, "root", "root")
                    && window_field_is(label, "window", second_id)
            }),
            "window-new does not hold the new window's shell-create{{root, window}}: {:#?}",
            trace.stacks_since(mark)
        );
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    assert!(
        !captured.lines.iter().any(|line| line.contains(": ERROR")),
        "unexpected ERRORs: {:#?}",
        captured.lines
    );
}
