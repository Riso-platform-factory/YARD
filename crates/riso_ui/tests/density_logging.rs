use gpui::TestAppContext;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
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
            "riso-ui-density-{}-{}-{nanos:x}",
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
    matches!(crate_of(target), "riso_ui" | "riso_settings")
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

// The density the settings provider gives Zed's components.
fn density(cx: &gpui::App) -> theme::UiDensity {
    theme::theme_settings(cx).ui_density(cx)
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
        riso_settings::init(cx);
        riso_settings::load(cx, settings.to_path_buf());
        riso_settings::watch(cx);
        riso_ui::init(cx);
        riso_ui::apply_theme_setting(cx);
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

fn assert_density_keys(group: &Group) {
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["density"],
        "unexpected density-apply fields: {:#?}",
        group.fields
    );
}

fn assert_density_group(group: &Group, entered: &BTreeSet<u64>, app_init_id: u64) {
    assert_group(group, entered, "density-apply");
    assert_eq!(
        group.parent,
        Some(app_init_id),
        "density-apply must sit inside the group open when apply_theme_setting is called"
    );
    assert_density_keys(group);
}

fn assert_density_fields(group: &Group, density: &str) {
    assert_density_keys(group);
    assert_eq!(text(&group.fields["density"]), density);
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
// theme-apply{mode, theme}, ui-font-apply and density-apply{density} beside
// each other. Returns the settings-watch, settings-register and density-apply
// groups.
fn scenario_groups<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    app_init_id: u64,
    mode: &str,
    theme: &str,
) -> (&'a Group, &'a Group, &'a Group) {
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
    assert_group(font_g, &captured.entered_groups, "ui-font-apply");
    assert_eq!(
        font_g.parent, apply_g.parent,
        "ui-font-apply must sit beside theme-apply"
    );
    let density_g = only(applies, "density-apply");
    assert_density_group(density_g, &captured.entered_groups, app_init_id);
    assert_eq!(
        density_g.parent, apply_g.parent,
        "density-apply must sit beside theme-apply"
    );
    (watch_g, register_g, density_g)
}

// The groups of one change of the section while running: settings-reload{path}
// inside settings-watch, then theme-apply, ui-font-apply and
// density-apply{density} beside each other inside settings-watch.
// Returns the density-apply group.
fn change_groups<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    watch_id: u64,
) -> &'a Group {
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
    assert_eq!(
        reload_g.parent,
        Some(watch_id),
        "settings-reload must sit inside settings-watch"
    );
    let apply_g = only(applies, "theme-apply");
    assert_group(apply_g, &captured.entered_groups, "theme-apply");
    let font_g = only(applies, "ui-font-apply");
    assert_group(font_g, &captured.entered_groups, "ui-font-apply");
    let density_g = only(applies, "density-apply");
    assert_group(density_g, &captured.entered_groups, "density-apply");
    assert_density_keys(density_g);
    for group in [apply_g, font_g, density_g] {
        assert_eq!(
            group.parent,
            Some(watch_id),
            "{} must sit inside settings-watch",
            group.name
        );
    }
    density_g
}

// The one group of a change whose section cannot be read:
// settings-reload{path} inside settings-watch, and no apply group beside it.
fn unreadable_change_group<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    watch_id: u64,
) -> &'a Group {
    let [reload_g] = &captured.groups[cursor..] else {
        panic!(
            "expected settings-reload alone, got {:#?}",
            &captured.groups[cursor..]
        );
    };
    assert_path_group(reload_g, &captured.entered_groups, "settings-reload", path);
    assert_eq!(
        reload_g.parent,
        Some(watch_id),
        "settings-reload must sit inside settings-watch"
    );
    reload_g
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
fn density_opens_density_apply_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let mut groups_cursor = 0;
        let mut events_cursor = 0;
        let mut diagnostics_cursor = 0;

        // 1. No file: the defaults hold — the default density.
        //    density-apply{density=default} inside app-init, `applied` at DEBUG.
        let missing = temp.path().join("missing.json");
        let app_init = cx.update(|cx| startup(cx, &missing, None));
        cx.read(|cx| assert_eq!(density(cx), theme::UiDensity::Default));
        assert!(!missing.exists());
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &missing,
                app_init,
                "system",
                "One Light",
            );
            assert_density_fields(density_g, "default");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 2. A section without `ui_density` is still no value:
        //    density-apply{density=default}, `applied`.
        let plain = temp.path().join("plain.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &plain, Some(r#"{"theme": {"mode": "light"}}"#)));
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Default));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &plain,
                app_init,
                "light",
                "One Light",
            );
            assert_density_fields(density_g, "default");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 3. `compact` applies: density-apply{density=compact}, `applied`.
        let compact = temp.path().join("compact.json");
        let app = cx.new_app();
        let app_init = app.update(|cx| {
            startup(
                cx,
                &compact,
                Some(r#"{"theme": {"ui_density": "compact"}}"#),
            )
        });
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Compact));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &compact,
                app_init,
                "system",
                "One Light",
            );
            assert_density_fields(density_g, "compact");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 4. `comfortable` applies: density-apply{density=comfortable}, `applied`.
        let comfortable = temp.path().join("comfortable.json");
        let app = cx.new_app();
        let app_init = app.update(|cx| {
            startup(
                cx,
                &comfortable,
                Some(r#"{"theme": {"ui_density": "comfortable"}}"#),
            )
        });
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Comfortable));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &comfortable,
                app_init,
                "system",
                "One Light",
            );
            assert_density_fields(density_g, "comfortable");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 5. A value that is none of the three makes the section unreadable at
        //    startup: the settings engine's WARN `section theme cannot be read`
        //    inside settings-register{section=theme}, then `applied` for the
        //    defaults in density-apply{density=default} inside app-init.
        let huge = temp.path().join("huge.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &huge, Some(r#"{"theme": {"ui_density": "huge"}}"#)));
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Default));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, register_g, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &huge,
                app_init,
                "system",
                "One Light",
            );
            assert_density_fields(density_g, "default");
            let applied = assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(register_g.id));
            let Some(reason) = warn.message.strip_prefix("section theme cannot be read: ") else {
                panic!("unexpected diagnostic message: {}", warn.message);
            };
            assert!(!reason.trim().is_empty());
            assert!(
                warn.seq < applied.seq,
                "the settings engine's WARN must precede `applied`"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 6. An edit of the density while running applies at once: a new
        //    density-apply{density=compact} inside settings-watch, `applied`.
        let edited = temp.path().join("edited.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &edited, Some(r#"{"theme": {"ui_density": "default"}}"#)));
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Default));
        let watch_id;
        {
            let captured = CAPTURED.lock().unwrap();
            let (watch_g, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &edited,
                app_init,
                "system",
                "One Light",
            );
            watch_id = watch_g.id;
            assert_density_fields(density_g, "default");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(&edited, r#"{"theme": {"ui_density": "compact"}}"#).unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Compact));
        {
            let captured = CAPTURED.lock().unwrap();
            let density_g = change_groups(&captured, groups_cursor, &edited, watch_id);
            assert_density_fields(density_g, "compact");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 7. An edit that changes only the theme leaves the density as it was:
        //    `unchanged` in density-apply{density=compact} inside
        //    settings-watch, while theme-apply applies One Dark beside it.
        let same = temp.path().join("same.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &same, Some(r#"{"theme": {"ui_density": "compact"}}"#)));
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Compact));
        let watch_id;
        {
            let captured = CAPTURED.lock().unwrap();
            let (watch_g, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &same,
                app_init,
                "system",
                "One Light",
            );
            watch_id = watch_g.id;
            assert_density_fields(density_g, "compact");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(
            &same,
            r#"{"theme": {"mode": "dark", "ui_density": "compact"}}"#,
        )
        .unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| {
            assert_eq!(riso_ui::active_theme(cx), "One Dark");
            assert_eq!(density(cx), theme::UiDensity::Compact);
        });
        {
            let captured = CAPTURED.lock().unwrap();
            let density_g = change_groups(&captured, groups_cursor, &same, watch_id);
            assert_density_fields(density_g, "compact");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "unchanged",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 8. An edit to a value that is none of the three cannot be read: the
        //    settings engine's WARN `section theme cannot be read` inside
        //    settings-reload, no apply group at all, and the last good density
        //    stays.
        let broken = temp.path().join("broken.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &broken, Some(r#"{"theme": {"ui_density": "compact"}}"#)));
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Compact));
        let watch_id;
        {
            let captured = CAPTURED.lock().unwrap();
            let (watch_g, _, density_g) = scenario_groups(
                &captured,
                groups_cursor,
                &broken,
                app_init,
                "system",
                "One Light",
            );
            watch_id = watch_g.id;
            assert_density_fields(density_g, "compact");
            assert_only_event(
                &events_in_group(&captured, events_cursor, density_g.id),
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
        fs::write(&broken, r#"{"theme": {"ui_density": "huge"}}"#).unwrap();
        app.executor().advance_clock(Duration::from_secs(1));
        app.run_until_parked();
        app.read(|cx| assert_eq!(density(cx), theme::UiDensity::Compact));
        {
            let captured = CAPTURED.lock().unwrap();
            let reload_g = unreadable_change_group(&captured, groups_cursor, &broken, watch_id);
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(reload_g.id));
            let Some(reason) = warn.message.strip_prefix("section theme cannot be read: ") else {
                panic!("unexpected diagnostic message: {}", warn.message);
            };
            assert!(!reason.trim().is_empty());
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
}
