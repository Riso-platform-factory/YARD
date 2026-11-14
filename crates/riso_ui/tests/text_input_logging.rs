// Alone in this binary: captures tracing output, which would race with any
// other test reaching the same log lines.

use gpui::{
    AppContext, ClipboardItem, Entity, Focusable, ParentElement, Styled, TestAppContext, div, px,
};
use riso_ui::{TextInput, bind_text_input_keys, v_flex};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

struct Host {
    input: Entity<TextInput>,
}

impl gpui::Render for Host {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        v_flex()
            .size_full()
            .child(div().w(px(240.)).child(self.input.clone()))
    }
}

// The number of key bindings the contract's table gives for this OS.
#[cfg(target_os = "macos")]
const BINDINGS: usize = 31;
#[cfg(not(target_os = "macos"))]
const BINDINGS: usize = 23;

#[cfg(target_os = "macos")]
const KEYS: &[&str] = &[
    "backspace",
    "delete",
    "alt-backspace",
    "cmd-backspace",
    "left",
    "right",
    "alt-left",
    "alt-right",
    "cmd-left",
    "cmd-right",
    "home",
    "end",
    "ctrl-a",
    "ctrl-e",
    "shift-left",
    "shift-right",
    "alt-shift-left",
    "alt-shift-right",
    "cmd-shift-left",
    "cmd-shift-right",
    "shift-home",
    "shift-end",
    "ctrl-shift-a",
    "ctrl-shift-e",
    "cmd-a",
    "cmd-c",
    "cmd-x",
    "cmd-v",
    "cmd-z",
    "cmd-shift-z",
    "enter",
];

#[cfg(not(target_os = "macos"))]
const KEYS: &[&str] = &[
    "backspace",
    "delete",
    "ctrl-backspace",
    "left",
    "right",
    "ctrl-left",
    "ctrl-right",
    "home",
    "end",
    "shift-left",
    "shift-right",
    "ctrl-shift-left",
    "ctrl-shift-right",
    "shift-home",
    "shift-end",
    "ctrl-a",
    "ctrl-c",
    "ctrl-x",
    "ctrl-v",
    "ctrl-z",
    "ctrl-shift-z",
    "ctrl-y",
    "enter",
];

#[cfg(target_os = "macos")]
const CLIPBOARD_KEYS: (&str, &str, &str, &str) = ("cmd-a", "cmd-c", "cmd-x", "cmd-v");
#[cfg(not(target_os = "macos"))]
const CLIPBOARD_KEYS: (&str, &str, &str, &str) = ("ctrl-a", "ctrl-c", "ctrl-x", "ctrl-v");

#[derive(Default)]
struct Captured {
    // Every span by id, as `LEVEL name fields`, with the id of the span it was
    // entered under, if any.
    spans: BTreeMap<u64, (String, Option<u64>)>,
    entered: Vec<u64>,
    // Every event, as `innermost span: LEVEL target fields`.
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        write!(self.0, " {}={value}", field.name()).unwrap();
    }

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
        let parent = captured.entered.last().copied();
        captured.spans.insert(
            id,
            (
                format!(
                    "{} {}{}",
                    attributes.metadata().level(),
                    attributes.metadata().name(),
                    fields.0
                ),
                parent,
            ),
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let group = captured
            .entered
            .last()
            .and_then(|id| captured.spans.get(id))
            .map_or_else(|| "none".to_string(), |(span, _)| span.clone());
        captured.lines.push(format!(
            "{group}: {} {}{}",
            event.metadata().level(),
            event.metadata().target(),
            fields.0
        ));
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

fn lines_at_level(captured: &Captured, level: &str, message: &str) -> Vec<String> {
    captured
        .lines
        .iter()
        .filter(|line| line.contains(level) && line.contains(message))
        .cloned()
        .collect()
}

#[gpui::test]
fn the_input_logs_its_bindings_each_change_the_confirm_and_the_clipboard(cx: &mut TestAppContext) {
    cx.update(riso_ui::init);
    let (host, cx) = cx.add_window_view(|window, cx| Host {
        input: cx.new(|cx| TextInput::new(window, cx)),
    });
    let input = host.read_with(cx, |host, _| host.input.clone());
    cx.update(|window, cx| input.focus_handle(cx).focus(window, cx));
    let trace = Trace::default();

    // The keys are bound inside `app-init`: one group carrying the count, with
    // one debug line per binding.
    tracing::subscriber::with_default(trace.clone(), || {
        let app_init = tracing::info_span!("app-init");
        let _entered = app_init.entered();
        cx.update(|_, cx| bind_text_input_keys(cx));
    });
    {
        let captured = trace.0.lock().unwrap();
        let (_, (group, parent)) = captured
            .spans
            .iter()
            .find(|(_, (name, _))| name.contains("text-input-keys"))
            .expect("binding the keys opens the text-input-keys group");
        assert!(
            group.contains(&format!("count={BINDINGS}")),
            "the group carries the number of bindings: {group}"
        );
        assert!(
            parent
                .and_then(|id| captured.spans.get(&id))
                .is_some_and(|(name, _)| name.contains("app-init")),
            "the group is inside app-init: {group} under {parent:?}"
        );
        let inside: Vec<_> = captured
            .lines
            .iter()
            .filter(|line| line.starts_with(&format!("{group}: ")))
            .collect();
        assert_eq!(
            inside.len(),
            BINDINGS,
            "one debug line per binding inside the group: {inside:?}"
        );
        assert!(
            inside.iter().all(|line| line.contains(": DEBUG ")),
            "each binding line is a debug line: {inside:?}"
        );
        for key in KEYS {
            assert!(
                inside.iter().any(|line| line.contains(key)),
                "a binding line names {key}: {inside:?}"
            );
        }
    }
    trace.0.lock().unwrap().lines.clear();

    // Each change logs `text changed` at trace with the text's length; moves
    // log none.
    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_input("hi");
    });
    {
        let captured = trace.0.lock().unwrap();
        let changed = lines_at_level(&captured, ": TRACE ", "text changed");
        assert_eq!(
            changed.len(),
            2,
            "one 'text changed' per typed character: {changed:?}"
        );
        assert!(changed[0].contains("length=1"), "{changed:?}");
        assert!(changed[1].contains("length=2"), "{changed:?}");
    }
    trace.0.lock().unwrap().lines.clear();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_keystrokes("left right backspace");
    });
    {
        let captured = trace.0.lock().unwrap();
        let changed = lines_at_level(&captured, ": TRACE ", "text changed");
        assert_eq!(
            changed.len(),
            1,
            "the moves log nothing; the backspace changes once: {changed:?}"
        );
        assert!(changed[0].contains("length=1"), "{changed:?}");
    }
    trace.0.lock().unwrap().lines.clear();

    // Enter logs `text confirmed` at debug with the text's length.
    tracing::subscriber::with_default(trace.clone(), || {
        cx.simulate_keystrokes("enter");
    });
    {
        let captured = trace.0.lock().unwrap();
        let confirmed = lines_at_level(&captured, ": DEBUG ", "text confirmed");
        assert_eq!(
            confirmed.len(),
            1,
            "enter logs 'text confirmed' once: {:?}",
            captured.lines
        );
        assert!(confirmed[0].contains("length=1"), "{confirmed:?}");
    }
    trace.0.lock().unwrap().lines.clear();

    // Copy, cut, and paste each log at debug with the length. The clipboard is
    // seeded after the cut so the paste has a length of its own.
    tracing::subscriber::with_default(trace.clone(), || {
        let (all, copy, cut, paste) = CLIPBOARD_KEYS;
        cx.simulate_keystrokes(all);
        cx.simulate_keystrokes(copy);
        cx.simulate_keystrokes(cut);
        cx.write_to_clipboard(ClipboardItem::new_string("pq".into()));
        cx.simulate_keystrokes(paste);
    });
    {
        let captured = trace.0.lock().unwrap();
        for (message, length) in [("copied", 1), ("cut", 1), ("pasted", 2)] {
            let lines = lines_at_level(&captured, ": DEBUG ", message);
            assert_eq!(
                lines.len(),
                1,
                "one '{message}' debug line: {:?}",
                captured.lines
            );
            assert!(lines[0].contains(&format!("length={length}")), "{lines:?}");
        }
    }
}
