#![cfg(target_os = "macos")]

use gpui::TestAppContext;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-ui-font-{}-{}-{nanos:x}",
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

#[derive(Debug)]
struct Group {
    id: u64,
    parent: Option<u64>,
    level: tracing::Level,
    name: &'static str,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Event {
    seq: u64,
    group: Option<u64>,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Diagnostic {
    seq: u64,
    level: log::Level,
    group: Option<u64>,
    message: String,
}

struct Captured {
    next_id: u64,
    next_seq: u64,
    spans: BTreeMap<u64, &'static str>,
    entered: Vec<u64>,
    entered_groups: BTreeSet<u64>,
    groups: Vec<Group>,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
}

impl Captured {
    const fn new() -> Self {
        Self {
            next_id: 1,
            next_seq: 1,
            spans: BTreeMap::new(),
            entered: Vec::new(),
            entered_groups: BTreeSet::new(),
            groups: Vec::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    // The innermost entered span of the same crate: the group open at that moment.
    fn current_group(&self, target: &str) -> Option<u64> {
        let krate = crate_of(target);
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| crate_of(self.spans[id]) == krate)
    }
}

fn crate_of(target: &str) -> &str {
    target.split("::").next().unwrap_or(target)
}

fn watched(target: &str) -> bool {
    matches!(crate_of(target), "yard_ui" | "yard_settings")
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        watched(metadata.target())
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_group(record.metadata().target());
        captured.diagnostics.push(Diagnostic {
            seq,
            level: record.level(),
            group,
            message,
        });
    }

    fn flush(&self) {}
}

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

struct Trace;

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
        let metadata = attributes.metadata();
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        let parent = if attributes.is_contextual() {
            captured.entered.last().copied()
        } else {
            attributes.parent().map(|id| id.into_u64())
        };
        if watched(metadata.target()) {
            captured.groups.push(Group {
                id,
                parent,
                level: *metadata.level(),
                name: metadata.name(),
                fields: fields.values,
            });
        }
        captured.spans.insert(id, metadata.target());
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, id: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let id = id.into_u64();
        if let Some(group) = captured.groups.iter_mut().find(|group| group.id == id) {
            group.fields.extend(fields.values);
        }
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !watched(metadata.target()) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_group(metadata.target());
        captured.events.push(Event {
            seq,
            group,
            level: *metadata.level(),
            message: fields.message,
            fields: fields.values,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        let id = id.into_u64();
        let mut captured = CAPTURED.lock().unwrap();
        if watched(captured.spans[&id]) {
            captured.entered_groups.insert(id);
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(CAPTURED.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

// A field's text, however it was recorded (`str`, `%`, or `?`).
fn text(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

// The size field holds a number of pixels, however it was recorded.
fn pixels(value: &str) -> f32 {
    text(value)
        .trim_end_matches("px")
        .trim_end()
        .parse()
        .unwrap_or_else(|_| panic!("expected a size in pixels, got {value}"))
}

// A test app backed by the machine's text system, so that installed families —
// Menlo among them, and IBM Plex Sans once the bundled fonts register — are
// known. The default test text system knows no font at all.
fn fonts_app(cx: &TestAppContext) -> TestAppContext {
    TestAppContext::build_with_text_system(
        cx.dispatcher.clone(),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    )
}

// The startup's order: the store, the file, the watch, the UI, then the theme
// setting, inside the group that surrounds them, as `app-init` does in the
// application.
fn startup(cx: &mut gpui::App, settings: &Path, content: Option<&str>) -> u64 {
    if let Some(content) = content {
        fs::write(settings, content).unwrap();
    }
    let app_init = tracing::info_span!("app-init");
    let app_init_id = app_init.id().unwrap().into_u64();
    {
        let _group = app_init.entered();
        yard_settings::init(cx);
        yard_settings::load(cx, settings.to_path_buf());
        yard_settings::watch(cx);
        yard_ui::init(cx);
        yard_ui::apply_theme_setting(cx);
    }
    app_init_id
}

fn assert_group(group: &Group, entered: &BTreeSet<u64>, name: &str) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, name);
}

fn assert_path_group(group: &Group, entered: &BTreeSet<u64>, name: &str, expected_path: &Path) {
    assert_group(group, entered, name);
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected {name} fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected {name} path {display} (or {debug}), got {actual}"
    );
}

fn assert_apply_group(
    group: &Group,
    entered: &BTreeSet<u64>,
    app_init_id: u64,
    mode: &str,
    theme: &str,
) {
    assert_group(group, entered, "theme-apply");
    assert_eq!(
        group.parent,
        Some(app_init_id),
        "theme-apply must sit inside the group open when apply_theme_setting is called"
    );
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["mode", "theme"],
        "unexpected theme-apply fields: {:#?}",
        group.fields
    );
    assert_eq!(text(&group.fields["mode"]), mode);
    assert_eq!(text(&group.fields["theme"]), theme);
}

fn assert_font_keys(group: &Group) {
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["family", "size"],
        "unexpected ui-font-apply fields: {:#?}",
        group.fields
    );
}

fn assert_font_group(group: &Group, entered: &BTreeSet<u64>, app_init_id: u64) {
    assert_group(group, entered, "ui-font-apply");
    assert_eq!(
        group.parent,
        Some(app_init_id),
        "ui-font-apply must sit inside the group open when apply_theme_setting is called"
    );
    assert_font_keys(group);
}

fn assert_font_fields(group: &Group, family: &str, size: f32) {
    assert_eq!(text(&group.fields["family"]), family);
    assert_eq!(pixels(&group.fields["size"]), size);
}

// The one group of `name` in `rest`.
fn only<'a>(rest: &'a [Group], name: &str) -> &'a Group {
    let mut found = rest.iter().filter(|group| group.name == name);
    let Some(group) = found.next() else {
        panic!("no {name} group in {rest:#?}");
    };
    assert!(
        found.next().is_none(),
        "more than one {name} group in {rest:#?}"
    );
    group
}

// The eight groups of one dev startup: settings-init, settings-load,
// settings-watch, ui-init, settings-register{section=theme}, then
// theme-apply{mode, theme}, ui-font-apply{family, size} and
// density-apply{density} beside each other.
// Returns the theme-apply and ui-font-apply groups.
fn scenario_groups<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    app_init_id: u64,
    mode: &str,
    theme: &str,
) -> (&'a Group, &'a Group) {
    let [init_g, load_g, watch_g, ui_init_g, register_g, applies @ ..] = &captured.groups[cursor..]
    else {
        panic!("expected 8 groups, got {:#?}", &captured.groups[cursor..]);
    };
    assert_eq!(
        applies.len(),
        3,
        "expected theme-apply, ui-font-apply and density-apply, got {applies:#?}"
    );
    assert_group(init_g, &captured.entered_groups, "settings-init");
    assert!(
        init_g.fields.is_empty(),
        "unexpected settings-init fields: {:#?}",
        init_g.fields
    );
    assert_path_group(load_g, &captured.entered_groups, "settings-load", path);
    assert_group(watch_g, &captured.entered_groups, "settings-watch");
    assert!(
        watch_g.fields.is_empty(),
        "unexpected settings-watch fields: {:#?}",
        watch_g.fields
    );
    assert_group(ui_init_g, &captured.entered_groups, "ui-init");
    assert!(
        ui_init_g.fields.is_empty(),
        "unexpected ui-init fields: {:#?}",
        ui_init_g.fields
    );
    assert_group(register_g, &captured.entered_groups, "settings-register");
    assert_eq!(
        register_g.fields,
        BTreeMap::from([("section", "theme".to_owned())])
    );
    let apply_g = only(applies, "theme-apply");
    assert_apply_group(apply_g, &captured.entered_groups, app_init_id, mode, theme);
    let font_g = only(applies, "ui-font-apply");
    assert_font_group(font_g, &captured.entered_groups, app_init_id);
    assert_eq!(
        font_g.parent, apply_g.parent,
        "ui-font-apply must sit beside theme-apply"
    );
    let density_g = only(applies, "density-apply");
    assert_group(density_g, &captured.entered_groups, "density-apply");
    assert_eq!(
        density_g.parent, apply_g.parent,
        "density-apply must sit beside theme-apply"
    );
    (apply_g, font_g)
}

// The groups of one change of the section while running: settings-reload{path},
// then theme-apply, ui-font-apply and density-apply beside each other.
// Returns the theme-apply and ui-font-apply groups.
fn change_groups<'a>(captured: &'a Captured, cursor: usize, path: &Path) -> (&'a Group, &'a Group) {
    let [reload_g, applies @ ..] = &captured.groups[cursor..] else {
        panic!(
            "expected settings-reload, theme-apply, ui-font-apply and density-apply, got {:#?}",
            &captured.groups[cursor..]
        );
    };
    assert_eq!(
        applies.len(),
        3,
        "expected theme-apply, ui-font-apply and density-apply, got {applies:#?}"
    );
    assert_path_group(reload_g, &captured.entered_groups, "settings-reload", path);
    let apply_g = only(applies, "theme-apply");
    assert_group(apply_g, &captured.entered_groups, "theme-apply");
    let font_g = only(applies, "ui-font-apply");
    assert_group(font_g, &captured.entered_groups, "ui-font-apply");
    assert_eq!(
        font_g.parent, apply_g.parent,
        "ui-font-apply must sit beside theme-apply"
    );
    assert_font_keys(font_g);
    let density_g = only(applies, "density-apply");
    assert_group(density_g, &captured.entered_groups, "density-apply");
    assert_eq!(
        density_g.parent, apply_g.parent,
        "density-apply must sit beside theme-apply"
    );
    (apply_g, font_g)
}

fn events_in_group(captured: &Captured, cursor: usize, group: u64) -> Vec<&Event> {
    captured.events[cursor..]
        .iter()
        .filter(|event| event.group == Some(group))
        .collect()
}

fn assert_only_event<'a>(events: &[&'a Event], message: &str) -> &'a Event {
    assert_eq!(
        events.len(),
        1,
        "expected `{message}` alone in the group, got {events:#?}"
    );
    let event = events[0];
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some(message));
    assert!(
        event.fields.is_empty(),
        "unexpected {message} fields: {:#?}",
        event.fields
    );
    event
}

fn diagnostics_since(captured: &Captured, cursor: usize, expected: usize) -> &[Diagnostic] {
    let diagnostics = &captured.diagnostics[cursor..];
    assert_eq!(
        diagnostics.len(),
        expected,
        "expected {expected} diagnostics, got {diagnostics:#?}"
    );
    diagnostics
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn ui_font_opens_ui_font_apply_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let mut groups_cursor = 0;
        let mut events_cursor = 0;
        let mut diagnostics_cursor = 0;

        // 1. No file: the defaults hold — IBM Plex Sans at 16.
        //    ui-font-apply{family=IBM Plex Sans, size=16} inside app-init,
        //    `applied` at DEBUG.
        let missing = temp.path().join("missing.json");
        let app = fonts_app(cx);
        let app_init = app.update(|cx| startup(cx, &missing, None));
        app.read(|cx| {
            assert_eq!(yard_ui::ui_font(cx).family, "IBM Plex Sans");
            assert_eq!(yard_ui::ui_font_size(cx), gpui::px(16.));
        });
        assert!(!missing.exists());
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &missing,
                app_init,
                "system",
                "One Light",
            );
            assert_font_fields(font_g, "IBM Plex Sans", 16.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 2. A size between 6 and 100 applies:
        //    ui-font-apply{family=IBM Plex Sans, size=20}, `applied`.
        let sized = temp.path().join("sized.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &sized, Some(r#"{"theme": {"ui_font_size": 20}}"#)));
        app.read(|cx| assert_eq!(yard_ui::ui_font_size(cx), gpui::px(20.)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &sized,
                app_init,
                "system",
                "One Light",
            );
            assert_font_fields(font_g, "IBM Plex Sans", 20.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 3. A size above 100 is clamped: the WARN
        //    `ui_font_size 200 is clamped to 100` inside ui-font-apply, then `applied`.
        let large = temp.path().join("large.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &large, Some(r#"{"theme": {"ui_font_size": 200}}"#)));
        app.read(|cx| assert_eq!(yard_ui::ui_font_size(cx), gpui::px(100.)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &large,
                app_init,
                "system",
                "One Light",
            );
            assert_font_keys(font_g);
            let applied = assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(font_g.id));
            assert_eq!(warn.message, "ui_font_size 200 is clamped to 100");
            assert!(
                warn.seq < applied.seq,
                "the clamp WARN must precede `applied`"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 4. A size below 6 is clamped the same way:
        //    `ui_font_size 2 is clamped to 6`, then `applied`.
        let small = temp.path().join("small.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &small, Some(r#"{"theme": {"ui_font_size": 2}}"#)));
        app.read(|cx| assert_eq!(yard_ui::ui_font_size(cx), gpui::px(6.)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &small,
                app_init,
                "system",
                "One Light",
            );
            assert_font_keys(font_g);
            let applied = assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(font_g.id));
            assert_eq!(warn.message, "ui_font_size 2 is clamped to 6");
            assert!(
                warn.seq < applied.seq,
                "the clamp WARN must precede `applied`"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 5. An installed family applies:
        //    ui-font-apply{family=Menlo, size=16}, `applied`.
        let menlo = temp.path().join("menlo.json");
        let app = fonts_app(cx);
        let app_init = app.update(|cx| {
            startup(
                cx,
                &menlo,
                Some(r#"{"theme": {"ui_font_family": "Menlo"}}"#),
            )
        });
        app.read(|cx| assert_eq!(yard_ui::ui_font(cx).family, "Menlo"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &menlo,
                app_init,
                "system",
                "One Light",
            );
            assert_font_fields(font_g, "Menlo", 16.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 6. A family the text system does not know falls back to the default,
        //    and the size still applies: the WARN `font Nope Sans is not
        //    installed` inside ui-font-apply.
        let nope = temp.path().join("nope.json");
        let app = fonts_app(cx);
        let app_init = app.update(|cx| {
            startup(
                cx,
                &nope,
                Some(r#"{"theme": {"ui_font_family": "Nope Sans", "ui_font_size": 20}}"#),
            )
        });
        app.read(|cx| {
            assert_eq!(yard_ui::ui_font(cx).family, "IBM Plex Sans");
            assert_eq!(yard_ui::ui_font_size(cx), gpui::px(20.));
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &nope,
                app_init,
                "system",
                "One Light",
            );
            assert_font_keys(font_g);
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(font_g.id));
            assert_eq!(warn.message, "font Nope Sans is not installed");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 7. An edit of the size while running applies at once: a new
        //    ui-font-apply beside theme-apply, with `applied`.
        let edited = temp.path().join("edited.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &edited, Some(r#"{"theme": {"ui_font_size": 20}}"#)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &edited,
                app_init,
                "system",
                "One Light",
            );
            assert_font_fields(font_g, "IBM Plex Sans", 20.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(&edited, r#"{"theme": {"ui_font_size": 24}}"#).unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| assert_eq!(yard_ui::ui_font_size(cx), gpui::px(24.)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = change_groups(&captured, groups_cursor, &edited);
            assert_font_fields(font_g, "IBM Plex Sans", 24.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 8. An edit that leaves family and size as they were does nothing:
        //    `unchanged` in ui-font-apply beside theme-apply.
        let same = temp.path().join("same.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &same, Some(r#"{"theme": {"ui_font_size": 20}}"#)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &same,
                app_init,
                "system",
                "One Light",
            );
            assert_font_fields(font_g, "IBM Plex Sans", 20.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(&same, r#"{"theme": {"mode": "dark", "ui_font_size": 20}}"#).unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| {
            assert_eq!(yard_ui::active_theme(cx), "One Dark");
            assert_eq!(yard_ui::ui_font(cx).family, "IBM Plex Sans");
            assert_eq!(yard_ui::ui_font_size(cx), gpui::px(20.));
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = change_groups(&captured, groups_cursor, &same);
            assert_font_fields(font_g, "IBM Plex Sans", 20.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "unchanged",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 9. A clamped size is reported once: the WARN fires at startup, and
        //    an edit that leaves family and size as they were logs
        //    `unchanged` and no WARN.
        let clamped = temp.path().join("clamped.json");
        let app = fonts_app(cx);
        let app_init =
            app.update(|cx| startup(cx, &clamped, Some(r#"{"theme": {"ui_font_size": 200}}"#)));
        app.read(|cx| assert_eq!(yard_ui::ui_font_size(cx), gpui::px(100.)));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &clamped,
                app_init,
                "system",
                "One Light",
            );
            assert_font_keys(font_g);
            let applied = assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "applied",
            );
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(font_g.id));
            assert_eq!(warn.message, "ui_font_size 200 is clamped to 100");
            assert!(
                warn.seq < applied.seq,
                "the clamp WARN must precede `applied`"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(
            &clamped,
            r#"{"theme": {"ui_font_size": 200, "mode": "dark"}}"#,
        )
        .unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| {
            assert_eq!(yard_ui::active_theme(cx), "One Dark");
            assert_eq!(yard_ui::ui_font_size(cx), gpui::px(100.));
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = change_groups(&captured, groups_cursor, &clamped);
            assert_font_fields(font_g, "IBM Plex Sans", 200.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "unchanged",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 10. The same with a family that is not installed: the WARN fires at
        //     startup, and the edit logs `unchanged` and no WARN.
        let uninstalled = temp.path().join("uninstalled.json");
        let app = fonts_app(cx);
        let app_init = app.update(|cx| {
            startup(
                cx,
                &uninstalled,
                Some(r#"{"theme": {"ui_font_family": "Nope Sans"}}"#),
            )
        });
        app.read(|cx| assert_eq!(yard_ui::ui_font(cx).family, "IBM Plex Sans"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = scenario_groups(
                &captured,
                groups_cursor,
                &uninstalled,
                app_init,
                "system",
                "One Light",
            );
            assert_font_keys(font_g);
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(font_g.id));
            assert_eq!(warn.message, "font Nope Sans is not installed");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(
            &uninstalled,
            r#"{"theme": {"ui_font_family": "Nope Sans", "mode": "dark"}}"#,
        )
        .unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| {
            assert_eq!(yard_ui::active_theme(cx), "One Dark");
            assert_eq!(yard_ui::ui_font(cx).family, "IBM Plex Sans");
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, font_g) = change_groups(&captured, groups_cursor, &uninstalled);
            assert_font_fields(font_g, "Nope Sans", 16.);
            assert_only_event(
                &events_in_group(&captured, events_cursor, font_g.id),
                "unchanged",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
}
