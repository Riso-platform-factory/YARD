use super::{TextInput, TextInputEvent, bind_text_input_keys};
use crate::v_flex;
use gpui::{
    AppContext, Entity, EntityInputHandler, Focusable, ParentElement, SharedString, Styled,
    TestAppContext, VisualTestContext, div, px, size,
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    sync::{Arc, Mutex},
};

// Every call to `set_value_from_assistive_technology` in this module runs under
// one lock and a scoped subscriber so no thread ever reaches its tracing
// callsites without a subscriber (no other unit test in `yard_ui` calls it).
static SET_VALUE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug)]
struct SpanRecord {
    level: String,
    name: String,
    fields: String,
}

#[derive(Clone, Debug)]
struct EventRecord {
    span_id: Option<u64>,
    level: String,
    target: String,
    fields: String,
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
    events: Vec<EventRecord>,
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
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let id = captured.spans.len() as u64 + 1;
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        captured.spans.insert(
            id,
            SpanRecord {
                level: attributes.metadata().level().to_string(),
                name: attributes.metadata().name().to_string(),
                fields: fields.0,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if let Some(record) = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .spans
            .get_mut(&span.into_u64())
        {
            record.fields.push_str(&fields.0);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let span_id = captured.entered.last().copied();
        captured.events.push(EventRecord {
            span_id,
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            fields: fields.0,
        });
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

struct InputHarness {
    input: Entity<TextInput>,
    changed: Rc<Cell<usize>>,
}

impl InputHarness {
    fn text(&self, cx: &mut VisualTestContext) -> String {
        self.input.read_with(cx, |input, _| input.text().to_owned())
    }

    fn selection(&self, cx: &mut VisualTestContext) -> Range<usize> {
        cx.update(|window, cx| {
            self.input.update(cx, |input, cx| {
                input
                    .selected_text_range(false, window, cx)
                    .expect("the input reports its selection")
                    .range
            })
        })
    }

    fn set_value_from_at(&self, value: &str, cx: &mut VisualTestContext) -> Trace {
        let _guard = SET_VALUE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let trace = Trace::default();
        let value = value.to_owned();
        tracing::subscriber::with_default(trace.clone(), || {
            cx.update(|window, cx| {
                self.input.update(cx, |input, cx| {
                    input.set_value_from_assistive_technology(value, window, cx);
                });
            });
        });
        assert!(
            trace
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
                .is_empty(),
            "every entered span exited cleanly"
        );
        trace
    }

    fn changed(&self) -> usize {
        self.changed.get()
    }
}

fn open_input(cx: &mut TestAppContext) -> (InputHarness, VisualTestContext) {
    cx.update(|cx| {
        crate::init(cx);
        bind_text_input_keys(cx);
    });
    let window = cx.open_window(size(px(400.), px(600.)), |window, cx| Host {
        input: cx.new(|cx| TextInput::new(window, cx)),
    });
    let input = window
        .root(cx)
        .expect("the window has a root view")
        .read_with(cx, |host, _| host.input.clone());
    let changed = Rc::new(Cell::new(0));
    let seen = changed.clone();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        cx.subscribe(&input, move |_, event: &TextInputEvent, _| {
            if matches!(event, TextInputEvent::Changed) {
                seen.set(seen.get() + 1);
            }
        })
        .detach();
        input.focus_handle(cx).focus(window, cx);
    });
    cx.run_until_parked();
    (InputHarness { input, changed }, cx)
}

#[gpui::test]
fn text_input_accessible_name_uses_label_then_placeholder_then_empty(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let window = cx.open_window(size(px(400.), px(200.)), |window, cx| Host {
        input: cx.new(|cx| TextInput::new(window, cx)),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let plain = cx.new(|cx| TextInput::new(window, cx));
        let placeholder_only =
            cx.new(|cx| TextInput::new(window, cx).placeholder("Type to search"));
        let label_only = cx.new(|cx| TextInput::new(window, cx).label("Search"));
        let both = cx.new(|cx| {
            TextInput::new(window, cx)
                .label("Search")
                .placeholder("Type to search")
        });
        let placeholder_then_label = cx.new(|cx| {
            TextInput::new(window, cx)
                .placeholder("Type to search")
                .label("Search")
        });

        let empty_name: SharedString = plain.read(cx).accessible_name();
        assert_eq!(empty_name.as_ref(), "");
        assert_eq!(
            placeholder_only.read(cx).accessible_name().as_ref(),
            "Type to search"
        );
        assert_eq!(label_only.read(cx).accessible_name().as_ref(), "Search");
        assert_eq!(both.read(cx).accessible_name().as_ref(), "Search");
        assert_eq!(
            placeholder_then_label.read(cx).accessible_name().as_ref(),
            "Search"
        );
    });
}

#[gpui::test]
fn set_value_from_assistive_technology_replaces_text_emits_once_and_places_caret_at_end(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open_input(cx);

    input.set_value_from_at("hello", &mut cx);
    assert_eq!(input.text(&mut cx), "hello");
    assert_eq!(input.changed(), 1, "setting the value emits Changed once");

    // The caret is at the end, so the next typed character lands at the end.
    cx.simulate_input("!");
    assert_eq!(input.text(&mut cx), "hello!");
    assert_eq!(input.changed(), 2);

    // Replacing existing text when the caret is in the middle also puts the caret at the end.
    cx.simulate_keystrokes("left left");
    input.set_value_from_at("world", &mut cx);
    assert_eq!(input.text(&mut cx), "world");
    assert_eq!(input.changed(), 3, "replacing the value emits Changed once");

    cx.simulate_input("?");
    assert_eq!(input.text(&mut cx), "world?");
    assert_eq!(input.changed(), 4);
}

#[gpui::test]
fn set_value_from_assistive_technology_with_the_same_value_changes_nothing_and_emits_no_event(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open_input(cx);

    input.set_value_from_at("", &mut cx);
    assert_eq!(input.text(&mut cx), "");
    assert_eq!(
        input.changed(),
        0,
        "setting empty on an empty input emits nothing"
    );

    input.set_value_from_at("same", &mut cx);
    assert_eq!(input.text(&mut cx), "same");
    assert_eq!(input.changed(), 1);

    input.set_value_from_at("same", &mut cx);
    assert_eq!(input.text(&mut cx), "same");
    assert_eq!(
        input.changed(),
        1,
        "setting the same value again emits no Changed event"
    );
}

#[gpui::test]
fn set_value_from_assistive_technology_with_the_same_value_keeps_caret_at_start(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open_input(cx);

    input.set_value_from_at("abc", &mut cx);
    assert_eq!(input.text(&mut cx), "abc");
    assert_eq!(input.changed(), 1);

    cx.simulate_keystrokes("home");
    assert_eq!(input.selection(&mut cx), 0..0);

    input.set_value_from_at("abc", &mut cx);
    assert_eq!(input.text(&mut cx), "abc");
    assert_eq!(
        input.changed(),
        1,
        "setting the same value again emits no Changed event"
    );
    assert_eq!(
        input.selection(&mut cx),
        0..0,
        "setting the same value again leaves the caret at the start"
    );

    cx.simulate_input("X");
    assert_eq!(input.text(&mut cx), "Xabc");
    assert_eq!(input.changed(), 2);
}

#[gpui::test]
fn set_value_from_assistive_technology_places_caret_at_end_for_multibyte_text(
    cx: &mut TestAppContext,
) {
    let (input, mut cx) = open_input(cx);
    let value = "héllo 🌍";

    input.set_value_from_at(value, &mut cx);
    assert_eq!(input.text(&mut cx), value);
    assert_eq!(input.changed(), 1, "setting the value emits Changed once");
    let end_utf16 = value.encode_utf16().count();
    assert_eq!(
        input.selection(&mut cx),
        end_utf16..end_utf16,
        "caret lands at the end of multi-byte text"
    );

    cx.simulate_input("!");
    assert_eq!(input.text(&mut cx), "héllo 🌍!");
    assert_eq!(input.changed(), 2);
}

#[gpui::test]
fn set_value_from_assistive_technology_logs_in_its_group_without_the_text(cx: &mut TestAppContext) {
    const SECRET: &str = "sentinel-a11y-secret-9f3c7e";

    let (input, mut cx) = open_input(cx);
    let trace = input.set_value_from_at(SECRET, &mut cx);

    let captured = trace.0.lock().unwrap_or_else(|error| error.into_inner());
    let (&group_id, group) = captured
        .spans
        .iter()
        .find(|(_, span)| span.name == "text-input-set-value")
        .unwrap_or_else(|| {
            panic!(
                "set_value_from_assistive_technology opens text-input-set-value: {:?}",
                captured.spans
            )
        });
    assert_eq!(
        group.level, "INFO",
        "text-input-set-value is an info span: {group:?}"
    );
    let expected_length = format!("length={}", SECRET.len());
    assert!(
        group.fields.contains(&expected_length),
        "text-input-set-value carries {expected_length}: {group:?}"
    );

    let set_events: Vec<_> = captured
        .events
        .iter()
        .filter(|event| {
            event.span_id == Some(group_id)
                && event.level == "DEBUG"
                && event.fields.contains("message=set")
        })
        .collect();
    assert_eq!(
        set_events.len(),
        1,
        "one debug!(\"set\") inside text-input-set-value: {:?}",
        captured.events
    );

    for span in captured.spans.values() {
        assert!(
            !span.name.contains(SECRET) && !span.fields.contains(SECRET),
            "span must not carry the input text: {span:?}"
        );
    }
    for event in &captured.events {
        assert!(
            !event.target.contains(SECRET) && !event.fields.contains(SECRET),
            "event must not carry the input text: {event:?}"
        );
    }
}
