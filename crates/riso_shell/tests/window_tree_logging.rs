#![cfg(feature = "dev")]

use gpui::{AppContext, InteractiveElement, IntoElement, Styled, TestAppContext, px};
use riso_shell::dev as shell;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs, with the records' groups.
const PROBE: &str = "window_tree_probe";

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
        if !(target.starts_with("riso_shell")
            || target.starts_with("riso_layout")
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
    fn stacks(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().span_stacks.clone()
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

// True when `line` holds `node` as a `node` field, quoted or not.
fn node_field(line: &str, node: &str) -> bool {
    line.contains(&format!("node=\"{node}\""))
        || line.contains(&format!("node={node} "))
        || line.ends_with(&format!("node={node}"))
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

// An operation that needs the window runs inside it.
fn window_update(
    window: gpui::WindowHandle<shell::Shell>,
    app: &mut TestAppContext,
    f: impl FnOnce(&mut gpui::Window, &mut gpui::App),
) {
    gpui::AnyWindowHandle::from(window)
        .update(app, |_, window, cx| f(window, cx))
        .unwrap();
}

// The window with the keyboard focus is the one calls act on.
fn activate(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) {
    window
        .update(app, |_, window, _| window.activate_window())
        .unwrap();
    app.run_until_parked();
}

fn window_id(window: gpui::WindowHandle<shell::Shell>, app: &mut TestAppContext) -> u64 {
    window
        .update(app, |_, window, _| {
            window.window_handle().window_id().as_u64()
        })
        .unwrap()
}

// A `shell-create` group names `id` as its `window`, with the root it placed.
fn shell_create_names(trace: &Trace, id: u64) -> bool {
    trace.stacks().iter().flatten().any(|label| {
        label.starts_with("shell-create")
            && field_is(label, "root", "root")
            && window_field_is(label, "window", id)
    })
}

// A span whose label starts with `group` was created as the operation's own
// group inside `window{id}` for `id`, in a stack opened since `mark`:
// `window{id}` wraps the whole call — it is the stack's first group and its
// only `window` group — so the operation's own groups inside it are
// unchanged.
fn inside_window(trace: &Trace, mark: usize, group: &str, id: u64) -> bool {
    trace.stacks_since(mark).iter().any(|stack| {
        let [window, operation] = stack.as_slice() else {
            return false;
        };
        window.starts_with("window ")
            && window_field_is(window, "id", id)
            && operation.starts_with(group)
    })
}

// A `window-close` group names `id` as its `window`, in a stack since `mark`.
fn window_close_names(trace: &Trace, mark: usize, id: u64) -> bool {
    trace
        .stacks_since(mark)
        .iter()
        .flatten()
        .any(|label| label.starts_with("window-close") && window_field_is(label, "window", id))
}

// The logging contract: each window's `shell-create` names its window; an
// operation's `layout-op` and a reset's `layout-reset` run inside `window{id}`
// with the window's own ID; a window's close opens `window-close{window}`
// with `view dropped` for each of its views; and a call with no window to act
// on logs `no window` at INFO in the call's group.
#[gpui::test]
fn every_windows_layout_work_runs_inside_its_own_window_group(cx: &mut TestAppContext) {
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
            host.place_root(cx, "root", "riso.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "b", "tour.card", "root", "children");
            host
        });
        let first = open_and_draw(&mut app);
        let second = open_and_draw(&mut app);
        let first_id = window_id(first, &mut app);
        let second_id = window_id(second, &mut app);
        for id in [first_id, second_id] {
            assert!(
                shell_create_names(&trace, id),
                "no shell-create group names window {id}"
            );
        }

        // A call without a window acts on the active window: its
        // `layout-op` runs inside `window{id}` with the active window's ID.
        activate(first, &mut app);
        let mark = trace.stack_mark();
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert!(
            inside_window(&trace, mark, "layout-op", first_id),
            "the hide's layout-op is not inside the first window's group"
        );

        // A call given a window acts on that window, even while the other
        // is active.
        activate(second, &mut app);
        let mark = trace.stack_mark();
        window_update(first, &mut app, |window, cx| {
            host.add(window, cx, "added", "tour.card", "root", "children")
        });
        assert!(
            inside_window(&trace, mark, "layout-op", first_id),
            "the add's layout-op followed the active window, not the given one"
        );

        // The active route with the second window.
        let mark = trace.stack_mark();
        app.update(|cx| host.set_hidden(cx, "a", true));
        assert!(
            inside_window(&trace, mark, "layout-op", second_id),
            "the hide's layout-op is not inside the second window's group"
        );

        // A reset's group is inside the window it was given too, even while
        // the other is active.
        activate(first, &mut app);
        let mark = trace.stack_mark();
        window_update(second, &mut app, |window, cx| host.reset_layout(window, cx));
        assert!(
            inside_window(&trace, mark, "layout-reset", second_id),
            "the reset's group is not inside the second window's group"
        );

        // Closing a window opens `window-close{window}` and logs `view
        // dropped` for each of its views inside it.
        let stacks = trace.stack_mark();
        let lines = trace.mark();
        second
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        assert!(
            window_close_names(&trace, stacks, second_id),
            "no window-close group names the second window"
        );
        let dropped = trace.since(lines);
        for node in ["a", "b"] {
            assert!(
                dropped.iter().any(|line| {
                    line.starts_with("window-close")
                        && window_field_is(line, "window", second_id)
                        && line.contains("view dropped")
                        && node_field(line, node)
                }),
                "the second window's close did not log `view dropped` for {node}: {dropped:#?}"
            );
        }

        // The first window's close drops its own views — `added` is its own.
        let lines = trace.mark();
        first
            .update(&mut app, |_, window, _| window.remove_window())
            .unwrap();
        app.run_until_parked();
        let dropped = trace.since(lines);
        assert!(
            dropped.iter().any(|line| {
                line.starts_with("window-close")
                    && window_field_is(line, "window", first_id)
                    && line.contains("view dropped")
                    && node_field(line, "added")
            }),
            "the first window's close did not log `view dropped` for `added`: {dropped:#?}"
        );

        // With no window left, a call is refused with `no window` in the
        // call's group.
        let mark = trace.mark();
        app.update(|cx| host.set_hidden(cx, "a", false));
        let lines = trace.since(mark);
        assert!(
            lines.iter().any(|line| {
                line.starts_with("layout-op")
                    && line.contains(": INFO")
                    && line.contains("no window")
            }),
            "a call with no window did not log `no window` at INFO: {lines:#?}"
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
