mod support;
use gpui::TestAppContext;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Captured {
    refreshes: Vec<String>,
    inspected: usize,
    disabled_labels: usize,
    observer_lines: Vec<String>,
    no_window: usize,
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
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        if span.metadata().name() == "inspector-refresh" {
            let mut fields = Fields::default();
            span.record(&mut fields);
            self.0.lock().unwrap().refreshes.push(fields.0);
        }
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if event.metadata().target().starts_with("yard_shell")
            && fields.0.contains("layout inspected")
        {
            self.0.lock().unwrap().inspected += 1;
        }
        if event.metadata().target().starts_with("yard_shell")
            && *event.metadata().level() == tracing::Level::INFO
            && fields.0 == " message=no window"
        {
            self.0.lock().unwrap().no_window += 1;
        }
        if fields.0.contains("layout observers") {
            self.0.lock().unwrap().observer_lines.push(fields.0.clone());
        }
        if event.metadata().target().starts_with("yard_ui")
            && fields.0.contains("rendering label")
            && fields.0.contains("color=Disabled")
        {
            self.0.lock().unwrap().disabled_labels += 1;
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// Alone in this binary: every draw and observed change uses this subscriber.
#[gpui::test]
fn refresh_is_one_group_per_change_and_draws_do_not_inspect(cx: &mut TestAppContext) {
    if !support::child("refresh_is_one_group_per_change_and_draws_do_not_inspect") {
        return;
    }
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        let (host, window) = support::setup(cx);
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = calls.clone();
        let _sentinel =
            cx.update(|cx| yard_shell::observe_layout(cx, move |_| counter.set(counter.get() + 1)));
        let before = trace.0.lock().unwrap().refreshes.len();
        cx.update(|cx| host.set_hidden(cx, "a", true));
        assert_eq!(&trace.0.lock().unwrap().refreshes[before..], [" nodes=5"]);
        let inspected = trace.0.lock().unwrap().inspected;
        let disabled_before = trace.0.lock().unwrap().disabled_labels;
        support::draw(window, cx);
        assert_eq!(
            trace.0.lock().unwrap().disabled_labels - disabled_before,
            3,
            "all three hidden-row labels use the kit's disabled color"
        );
        window.update(cx, |_, window, _| window.refresh()).unwrap();
        support::draw(window, cx);
        assert_eq!(
            trace.0.lock().unwrap().inspected,
            inspected,
            "the panel reads its snapshot on a frame"
        );
        assert_eq!(trace.0.lock().unwrap().refreshes.len(), before + 1);
        cx.update(|cx| host.remove(cx, "b"));
        assert_eq!(
            &trace.0.lock().unwrap().refreshes[before..],
            [" nodes=5", " nodes=4"]
        );
        assert_eq!(
            trace.0.lock().unwrap().observer_lines.last().unwrap(),
            " message=layout observers count=2"
        );
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        assert!(window.update(cx, |_, _, _| ()).is_err());
        let refreshes = trace.0.lock().unwrap().refreshes.len();
        let before_calls = calls.get();
        let observer_lines = trace.0.lock().unwrap().observer_lines.clone();
        let no_window = trace.0.lock().unwrap().no_window;
        cx.update(|cx| host.set_hidden(cx, "a", false));
        assert_eq!(calls.get(), before_calls);
        let captured = trace.0.lock().unwrap();
        assert_eq!(captured.observer_lines, observer_lines);
        assert_eq!(captured.no_window, no_window + 1);
        assert_eq!(
            captured.refreshes.len(),
            refreshes,
            "a closed window leaves no inspector panel listening"
        );
    });
}
