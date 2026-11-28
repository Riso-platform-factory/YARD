#![cfg(feature = "dev")]

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    StyleRefinement, Styled, TestAppContext, Window, div,
};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_i18n::{init, locale, on_locale_change, register, set_system_locale_for_tests, t};
use yard_settings::WriteOutcome;

static TRACE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    TRACE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug)]
struct SpanRecord {
    id: u64,
    level: tracing::Level,
    name: String,
    fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct EventRecord {
    level: tracing::Level,
    span_stack: Vec<u64>,
    message: String,
}

#[derive(Default)]
struct Captured {
    next_id: u64,
    spans: BTreeMap<u64, SpanRecord>,
    entered: Vec<u64>,
    entered_spans: BTreeSet<u64>,
    step_span_ids: Vec<u64>,
    step_events: Vec<EventRecord>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

impl Trace {
    fn take_step(&self) -> (Vec<SpanRecord>, Vec<EventRecord>) {
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let span_ids = std::mem::take(&mut captured.step_span_ids);
        let spans = span_ids
            .into_iter()
            .filter_map(|id| captured.spans.get(&id).cloned())
            .collect();
        let events = std::mem::take(&mut captured.step_events);
        (spans, events)
    }

    fn was_entered(&self, span_id: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entered_spans
            .contains(&span_id)
    }
}

#[derive(Default)]
struct FieldMap(BTreeMap<String, String>);

impl tracing::field::Visit for FieldMap {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_owned(),
            format!("{value:?}").trim_matches('"').to_owned(),
        );
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
        let mut fields = FieldMap::default();
        attributes.record(&mut fields);
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        captured.next_id += 1;
        let id = captured.next_id;
        let metadata = attributes.metadata();
        let record = SpanRecord {
            id,
            level: *metadata.level(),
            name: metadata.name().to_owned(),
            fields: fields.0,
        };
        captured.spans.insert(id, record);
        if metadata.target().starts_with("yard_i18n") {
            captured.step_span_ids.push(id);
        }
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("yard_i18n") {
            return;
        }
        let mut fields = FieldMap::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let span_stack = captured.entered.clone();
        captured.step_events.push(EventRecord {
            level: *metadata.level(),
            span_stack,
            message,
        });
    }

    fn enter(&self, span: &tracing::span::Id) {
        let id = span.into_u64();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        captured.entered.push(id);
        captured.entered_spans.insert(id);
    }

    fn exit(&self, span: &tracing::span::Id) {
        let id = span.into_u64();
        let mut captured = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = captured.entered.iter().rposition(|&open| open == id) {
            captured.entered.remove(pos);
        }
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-i18n-live-{}-{}-{nanos:x}",
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

struct GreetingView {
    shown: SharedString,
}

impl Render for GreetingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.shown = t(cx, "app.hello");
        div().child(self.shown.clone())
    }
}

struct RootView {
    greeting: Entity<GreetingView>,
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            self.greeting
                .clone()
                .cached(StyleRefinement::default().size_full()),
        )
    }
}

fn open_greeting_window(cx: &mut TestAppContext) -> Entity<GreetingView> {
    let window = cx.add_window(|_, cx| RootView {
        greeting: cx.new(|_| GreetingView {
            shown: SharedString::default(),
        }),
    });
    cx.read(|cx| window.read(cx).unwrap().greeting.clone())
}

fn shown_text(greeting: &Entity<GreetingView>, cx: &TestAppContext) -> SharedString {
    cx.read(|cx| greeting.read(cx).shown.clone())
}

fn setup(cx: &mut TestAppContext, watch_file: bool) -> (TempDir, PathBuf) {
    let dir = TempDir::new();
    let path = dir.path().join("settings.json");
    fs::write(&path, "{}").unwrap();
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path.clone());
        if watch_file {
            yard_settings::watch(cx);
        }
        init(cx);
        set_system_locale_for_tests(cx, None);
        register(cx, "app", "en", r#"{"hello": "Hello"}"#).unwrap();
        register(cx, "app", "vi", r#"{"hello": "Xin chào"}"#).unwrap();
    });
    (dir, path)
}

#[gpui::test]
fn changing_language_through_settings_write_redraws_view_and_updates_locale(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let (_dir, _path) = setup(cx, false);
    let greeting = open_greeting_window(cx);

    cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Hello");

    // Writing `language.locale` updates `locale(cx)` and refreshes windows so
    // the cached child view re-renders with the new language without a restart.
    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(locale(cx).as_str(), "vi"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Xin chào");

    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!(null)));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Hello");
}

#[gpui::test]
fn changing_language_in_settings_file_reloads_and_redraws_view(cx: &mut TestAppContext) {
    let _guard = lock();
    let (_dir, path) = setup(cx, true);
    let greeting = open_greeting_window(cx);

    cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Hello");

    fs::write(&path, r#"{"language": {"locale": "vi"}}"#).unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| assert_eq!(locale(cx).as_str(), "vi"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Xin chào");

    fs::write(&path, "{}").unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
    assert_eq!(shown_text(&greeting, cx).as_str(), "Hello");
}

#[gpui::test]
fn on_locale_change_runs_once_per_effective_change_and_skips_unchanged_publications(
    cx: &mut TestAppContext,
) {
    let _guard = lock();
    let (_dir, path) = setup(cx, true);
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    cx.update(|cx| {
        let seen = seen.clone();
        on_locale_change(cx, move |cx| {
            seen.borrow_mut().push(locale(cx).to_string());
        });
    });

    assert!(seen.borrow().is_empty());

    // Setting publication that keeps the effective locale at "en" does not fire the listener.
    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("en")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    assert!(
        seen.borrow().is_empty(),
        "unchanged effective locale (null -> en) must not run listener: {:?}",
        seen.borrow()
    );

    // Unsupported language still resolves to "en".
    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("fr")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    assert!(
        seen.borrow().is_empty(),
        "unchanged effective locale (en -> fr/en) must not run listener: {:?}",
        seen.borrow()
    );

    // Effective change: "en" -> "vi".
    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), ["vi"]);

    // Regional setting that still resolves to "vi" does not fire the listener again.
    let outcome =
        cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi-VN")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), ["vi"]);

    // File reload with unchanged locale does not fire the listener.
    fs::write(&path, r#"{"language": {"locale": "vi-VN"}, "other": 1}"#).unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), ["vi"]);

    // Effective change back: "vi" -> "en".
    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!(null)));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), ["vi", "en"]);
}

#[gpui::test]
fn locale_apply_runs_on_settings_file_reload(cx: &mut TestAppContext) {
    let _guard = lock();
    let trace = Trace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        tracing::callsite::rebuild_interest_cache();
        let (_dir, path) = setup(cx, true);
        let _ = trace.take_step();

        fs::write(&path, r#"{"language": {"locale": "vi"}}"#).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();

        cx.read(|cx| assert_eq!(locale(cx).as_str(), "vi"));
        let (spans, events) = trace.take_step();
        let apply_span = spans
            .iter()
            .find(|span| span.name == "locale-apply")
            .unwrap_or_else(|| panic!("expected locale-apply span on file reload, got {spans:#?}"));
        assert_eq!(apply_span.level, tracing::Level::INFO);
        assert!(trace.was_entered(apply_span.id));
        let requested = apply_span
            .fields
            .get("requested")
            .unwrap_or_else(|| panic!("locale-apply missing requested: {:#?}", apply_span.fields));
        assert!(
            requested.contains("vi"),
            "expected requested to contain \"vi\", got {requested:?}"
        );
        assert_eq!(
            apply_span.fields.get("resolved").map(String::as_str),
            Some("vi")
        );
        assert!(
            events.iter().any(|event| {
                event.span_stack.contains(&apply_span.id)
                    && event.level == tracing::Level::DEBUG
                    && event.message == "applied"
            }),
            "expected debug!(\"applied\") inside locale-apply on file reload, got {events:#?}"
        );

        fs::write(&path, "{}").unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();

        cx.read(|cx| assert_eq!(locale(cx).as_str(), "en"));
        let (spans, events) = trace.take_step();
        let apply_span = spans
            .iter()
            .find(|span| span.name == "locale-apply")
            .unwrap_or_else(|| {
                panic!("expected locale-apply span on reset file reload, got {spans:#?}")
            });
        assert!(trace.was_entered(apply_span.id));
        assert_eq!(
            apply_span.fields.get("resolved").map(String::as_str),
            Some("en")
        );
        assert!(
            events.iter().any(|event| {
                event.span_stack.contains(&apply_span.id)
                    && event.level == tracing::Level::DEBUG
                    && event.message == "applied"
            }),
            "expected debug!(\"applied\") inside locale-apply on reset file reload, got {events:#?}"
        );
    });
}
