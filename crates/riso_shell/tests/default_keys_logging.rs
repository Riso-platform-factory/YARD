use gpui::TestAppContext;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

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
        captured.spans.insert(
            id,
            format!(
                "{} {}{}",
                attributes.metadata().level(),
                attributes.metadata().name(),
                fields.0
            ),
        );
        tracing::span::Id::from_u64(id)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("riso_shell")
            || *event.metadata().level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        // Every enclosing group, outermost first.
        let group = if captured.entered.is_empty() {
            String::from("none")
        } else {
            captured
                .entered
                .iter()
                .map(|id| captured.spans[id].as_str())
                .collect::<Vec<_>>()
                .join(" > ")
        };
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

/// The innermost group enclosing `line`, or "" when no group is open.
fn group_of(line: &str) -> &str {
    line.split_once(": ")
        .and_then(|(stack, _)| stack.rsplit(" > ").next())
        .unwrap_or("")
}

/// Every group enclosing `line`, outermost first, joined by " > ".
fn stack_of(line: &str) -> &str {
    line.split_once(": ").map(|(stack, _)| stack).unwrap_or("")
}

#[cfg(target_os = "macos")]
const BINDINGS: &[(&str, &str)] = &[
    ("shift-escape", "ToggleZoom"),
    ("cmd-}", "SelectNextTab"),
    ("alt-cmd-right", "SelectNextTab"),
    ("cmd-{", "SelectPreviousTab"),
    ("alt-cmd-left", "SelectPreviousTab"),
    ("cmd-k cmd-left", "FocusLeft"),
    ("cmd-k cmd-right", "FocusRight"),
    ("cmd-k cmd-up", "FocusUp"),
    ("cmd-k cmd-down", "FocusDown"),
    ("escape", "Dismiss"),
];

#[cfg(not(target_os = "macos"))]
const BINDINGS: &[(&str, &str)] = &[
    ("shift-escape", "ToggleZoom"),
    ("ctrl-pagedown", "SelectNextTab"),
    ("ctrl-pageup", "SelectPreviousTab"),
    ("ctrl-k ctrl-left", "FocusLeft"),
    ("ctrl-k ctrl-right", "FocusRight"),
    ("ctrl-k ctrl-up", "FocusUp"),
    ("ctrl-k ctrl-down", "FocusDown"),
    ("escape", "Dismiss"),
];

// Alone in this binary: all calls reaching the captured lines use one subscriber.
#[gpui::test]
fn bind_default_keys_logs_a_keys_bind_group_with_a_debug_line_per_binding(cx: &mut TestAppContext) {
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        // The startup binds the default keys inside `app-init`.
        let _group = tracing::info_span!("app-init").entered();
        cx.update(riso_shell::bind_default_keys);
    });
    let captured = trace.0.lock().unwrap();
    assert!(captured.entered.is_empty());
    let count = BINDINGS.len();
    let group = format!("INFO keys-bind count={count}");
    let spans: Vec<String> = captured
        .spans
        .values()
        .map(|span| span.replace('"', ""))
        .collect();
    assert_eq!(
        spans.iter().filter(|span| **span == group).count(),
        1,
        "one `{group}` group in {spans:#?}"
    );
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let stack = format!("INFO app-init > {group}");
    let inside: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| group_of(line) == group)
        .collect();
    assert_eq!(inside.len(), count, "lines inside `{group}`: {lines:#?}");
    for (keys, action) in BINDINGS {
        assert_eq!(
            inside
                .iter()
                .filter(|line| {
                    stack_of(line) == stack
                        && line.contains(": DEBUG")
                        && line.contains(keys)
                        && line.contains(action)
                        && line.contains(if *action == "Dismiss" {
                            "ModalLayer"
                        } else {
                            "Shell"
                        })
                })
                .count(),
            1,
            "`{group}` must name {keys} for {action} once in {inside:#?}"
        );
    }
    assert!(
        !lines.iter().any(|line| line.contains(": ERROR")),
        "unexpected error in {lines:#?}"
    );
}
