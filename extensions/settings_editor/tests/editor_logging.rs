use gpui::{AppContext, IntoElement, Styled, TestAppContext, VisualTestContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_settings::Settings;
use yard_shell::Extension;

#[cfg(target_os = "macos")]
const SELECT_ALL: &str = "cmd-a";
#[cfg(not(target_os = "macos"))]
const SELECT_ALL: &str = "ctrl-a";

const SECRET_INITIAL: &str = "editor-secret-initial-6a1f9c";
const SECRET_UPDATED: &str = "editor-secret-updated-4d8e2b";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-editor-logging-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    parent: Option<u64>,
    entered: bool,
    level: tracing::Level,
    name: &'static str,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Clone, Debug)]
struct EventRecord {
    stack: Vec<u64>,
    level: tracing::Level,
    target: &'static str,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
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
struct Fields {
    message: Option<String>,
    values: BTreeMap<&'static str, String>,
}

impl Fields {
    fn insert(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.values.insert(field.name(), value);
        }
    }
}

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.insert(field, value.to_owned());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.insert(field, value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.insert(field, value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.insert(field, value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.insert(field, format!("{value:?}"));
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
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let parent = captured.entered.last().copied();
        captured.spans.insert(
            id,
            SpanRecord {
                id,
                parent,
                entered: false,
                level: *attributes.metadata().level(),
                name: attributes.metadata().name(),
                fields: fields.values,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if let Some(record) = self.0.lock().unwrap().spans.get_mut(&span.into_u64()) {
            record.fields.extend(fields.values);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        let stack = captured.entered.clone();
        captured.events.push(EventRecord {
            stack,
            level: *event.metadata().level(),
            target: event.metadata().target(),
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = self.0.lock().unwrap();
        if let Some(span) = captured.spans.get_mut(&id) {
            span.entered = true;
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard") || metadata.target().starts_with("settings_editor")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Name,
    Date,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Advanced {
    flag: bool,
}

/// Test section
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    /// Show the preview.
    preview: bool,
    /// Sort order.
    sort: Order,
    /// Items per page.
    #[schemars(range(min = 1, max = 100))]
    page_size: u32,
    /// Greeting.
    greeting: String,
    /// Nested.
    advanced: Advanced,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            preview: false,
            sort: Order::Name,
            page_size: 10,
            greeting: "hi".into(),
            advanced: Advanced::default(),
        }
    }
}

impl Settings for Sample {
    const KEY: &'static str = "sample";
}

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

fn draw_frames(
    window: gpui::WindowHandle<yard_shell::Shell>,
    cx: &mut TestAppContext,
    frames: usize,
) {
    for _ in 0..frames {
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            })
            .unwrap();
        cx.run_until_parked();
    }
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn clean_field(value: &str) -> &str {
    value.trim_matches('"')
}

fn has_ancestor(spans: &BTreeMap<u64, SpanRecord>, start: u64, ancestor: u64) -> bool {
    let mut current = spans.get(&start).and_then(|span| span.parent);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = spans.get(&id).and_then(|span| span.parent);
    }
    false
}

// Alone in this binary: every captured callsite runs under one scoped subscriber.
#[gpui::test]
fn opening_changing_and_refusing_in_the_editor_log_contract_groups_without_values(
    cx: &mut TestAppContext,
) {
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {});
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(
        &settings_path,
        serde_json::to_vec(&json!({
            "sample": {
                "greeting": SECRET_INITIAL
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            yard_settings::init(cx);
            yard_settings::load(cx, settings_path.clone());
            yard_ui::init(cx);
            yard_shell::bind_default_keys(cx);
            yard_ui::bind_text_input_keys(cx);

            let editor = Extension::new(cx, "settings_editor").unwrap();
            settings_editor::init(cx, &editor);

            let sample = Extension::new(cx, "sample").unwrap();
            sample.register_editable_settings::<Sample>(cx);
            sample.register(cx, "sample.card", &[], |_, _, _, cx| {
                cx.new(|_| Card).into()
            });
            sample.place_root(cx, "root", "sample.card");
        });

        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
                .unwrap()
        });
        cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
        draw_frames(window, cx, 1);

        let mut visual = VisualTestContext::from_window(window.into(), cx);

        // 1. `settings-editor-open` with `opened`
        visual.dispatch_action(settings_editor::Open);
        draw_frames(window, cx, 2);
        click(&mut visual, "section:sample");
        draw_frames(window, cx, 1);

        // 2. `settings-editor-change{section, field}` around a write (with a secret value)
        click(&mut visual, "field:sample.greeting");
        draw_frames(window, cx, 1);
        visual.simulate_keystrokes(SELECT_ALL);
        visual.simulate_input(SECRET_UPDATED);
        visual.simulate_keystrokes("enter");
        draw_frames(window, cx, 2);

        // 3. `info` `refused` for `"0"`
        click(&mut visual, "field:sample.page_size");
        draw_frames(window, cx, 1);
        visual.simulate_keystrokes(SELECT_ALL);
        visual.simulate_input("0");
        visual.simulate_keystrokes("enter");
        draw_frames(window, cx, 2);
    });

    let captured = trace.0.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "every entered span exited cleanly: {:?}",
        captured.entered
    );

    // Check `settings-editor-open` with `opened`.
    let open_span = captured
        .spans
        .values()
        .find(|span| span.name == "settings-editor-open")
        .unwrap_or_else(|| {
            panic!(
                "expected settings-editor-open span in {:#?}",
                captured.spans
            )
        });
    assert!(open_span.entered, "settings-editor-open must be entered");
    assert_eq!(open_span.level, tracing::Level::INFO);
    assert!(
        captured.events.iter().any(|event| {
            event.stack.contains(&open_span.id)
                && event.level == tracing::Level::DEBUG
                && event.message.as_deref() == Some("opened")
        }),
        "expected DEBUG 'opened' inside settings-editor-open, got {:#?}",
        captured.events
    );

    // Check `settings-editor-change{section, field}` around a write.
    let change_span = captured
        .spans
        .values()
        .find(|span| {
            span.name == "settings-editor-change"
                && span.fields.get("section").map(|s| clean_field(s)) == Some("sample")
                && span.fields.get("field").map(|s| clean_field(s)) == Some("greeting")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected settings-editor-change{{section=sample, field=greeting}} in {:#?}",
                captured.spans
            )
        });
    assert!(
        change_span.entered,
        "settings-editor-change must be entered"
    );
    assert_eq!(change_span.level, tracing::Level::INFO);

    let write_span = captured
        .spans
        .values()
        .find(|span| {
            span.name == "settings-write"
                && span.fields.get("section").map(|s| clean_field(s)) == Some("sample")
                && span.fields.get("field").map(|s| clean_field(s)) == Some("greeting")
        })
        .unwrap_or_else(|| {
            panic!(
                "expected settings-write{{section=sample, field=greeting}} in {:#?}",
                captured.spans
            )
        });
    assert!(
        has_ancestor(&captured.spans, write_span.id, change_span.id),
        "settings-editor-change must wrap settings-write: change={change_span:#?}, write={write_span:#?}"
    );

    // Check `info` `refused` for `"0"`.
    assert!(
        captured.events.iter().any(|event| {
            event.target.starts_with("settings_editor")
                && event.level == tracing::Level::INFO
                && event.message.as_deref() == Some("refused")
                && event
                    .fields
                    .get("reason")
                    .is_some_and(|reason| !clean_field(reason).trim().is_empty())
        }),
        "expected INFO 'refused' with reason for '0', got {:#?}",
        captured.events
    );

    // Check no value appears in any span, event, or diagnostic.
    let diagnostics = DIAGNOSTICS.lock().unwrap();
    for secret in [SECRET_INITIAL, SECRET_UPDATED] {
        for span in captured.spans.values() {
            assert!(
                !span.name.contains(secret),
                "span name leaked setting value: {span:#?}"
            );
            for (name, value) in &span.fields {
                assert!(
                    !name.contains(secret) && !value.contains(secret),
                    "span field leaked setting value: {span:#?}"
                );
            }
        }
        for event in &captured.events {
            if let Some(message) = &event.message {
                assert!(
                    !message.contains(secret),
                    "event message leaked setting value: {event:#?}"
                );
            }
            for (name, value) in &event.fields {
                assert!(
                    !name.contains(secret) && !value.contains(secret),
                    "event field leaked setting value: {event:#?}"
                );
            }
        }
        for (_, message) in diagnostics.iter() {
            assert!(
                !message.contains(secret),
                "diagnostic leaked setting value: {message}"
            );
        }
    }
    assert!(
        diagnostics
            .iter()
            .all(|(level, _)| *level != log::Level::Error),
        "unexpected ERROR diagnostics: {diagnostics:#?}"
    );
}
