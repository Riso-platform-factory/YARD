use gpui::{AppContext, TestAppContext, px};
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use yard_shell as shell;

// The diagnostic log's records are re-emitted as tracing events under this
// target, so one subscriber sees both logs, with their groups.
const PROBE: &str = "inspect_probe";

// The only diagnostic record this test should see is an ERROR.
struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            assert_eq!(record.level(), log::Level::Error);
            tracing::error!(target: PROBE, "{}", record.args());
        }
    }
    fn flush(&self) {}
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

// Alone in this binary: every call reaching captured lines uses this
// subscriber. Calling the layout observers after a change logs `layout
// observers {count}` at `debug`, inside the change's own group.
#[gpui::test]
fn calling_the_observers_logs_their_count_inside_the_changes_group(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let mut app = cx.new_app();
        let host = app.update(|cx| {
            yard_ui::init(cx);
            let host = shell::Extension::new(cx, "tour").unwrap();
            host.register(cx, "tour.card", &[], |_, _, _, cx| {
                cx.new(|_| gpui::Empty).into()
            });
            host.place_root(cx, "root", "yard.row");
            host.place(cx, "a", "tour.card", "root", "children");
            host.place(cx, "b", "tour.card", "root", "children");
            host
        });
        let calls = Rc::new(Cell::new(0usize));
        let _one = app.update(|cx| {
            let calls = calls.clone();
            shell::observe_layout(cx, move |_| calls.set(calls.get() + 1))
        });
        let _window = open_and_draw(&mut app);

        // One observer: the hide operation's own group holds `count=1`.
        let before = trace.mark();
        let before_calls = calls.get();
        app.update(|cx| host.set_hidden(cx, "a", true));
        let expected = r#"layout-op op="hide" node="a" extension="tour": DEBUG message=layout observers count=1"#;
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| line == expected),
            "missing {expected} in {lines:#?}"
        );
        assert_eq!(calls.get(), before_calls + 1);

        // A second registered observer joins the count.
        let _two = app.update(|cx| shell::observe_layout(cx, |_| {}));
        let before = trace.mark();
        let before_calls = calls.get();
        app.update(|cx| host.set_hidden(cx, "a", false));
        let expected = r#"layout-op op="show" node="a" extension="tour": DEBUG message=layout observers count=2"#;
        let lines = trace.since(before);
        assert!(
            lines.iter().any(|line| line == expected),
            "missing {expected} in {lines:#?}"
        );
        assert_eq!(calls.get(), before_calls + 1);
        assert!(trace.0.lock().unwrap().entered.is_empty());
    });
}
