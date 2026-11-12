use gpui::TestAppContext;
use riso_ui::{active_theme, apply_theme_setting, follow_appearance};
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
            "riso-ui-system-appearance-{}-{}-{nanos:x}",
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

struct Captured {
    next_id: u64,
    next_seq: u64,
    spans: BTreeMap<u64, &'static str>,
    entered: Vec<u64>,
    entered_groups: BTreeSet<u64>,
    groups: Vec<Group>,
    events: Vec<Event>,
    diagnostics: Vec<String>,
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
        let group = captured.current_group(record.metadata().target());
        captured
            .diagnostics
            .push(format!("{} {group:?}: {message}", record.level()));
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

// The startup's order: the store, the file, the watch, the UI, then the theme
// setting, inside the group that surrounds them, as `app-init` does in the
// application. `init` records the test platform's light, so a dark startup
// records dark again before the setting applies.
fn startup(cx: &mut gpui::App, settings: &Path, appearance: theme::Appearance) -> u64 {
    let app_init = tracing::info_span!("app-init");
    let app_init_id = app_init.id().unwrap().into_u64();
    {
        let _group = app_init.entered();
        riso_settings::init(cx);
        riso_settings::load(cx, settings.to_path_buf());
        riso_settings::watch(cx);
        riso_ui::init(cx);
        theme::SystemAppearance::global_mut(cx).0 = appearance;
        apply_theme_setting(cx);
    }
    app_init_id
}

// One second of the test clock runs the watch's next check.
fn tick(cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
}

// A window the theme setting follows: opening it reports the OS's appearance.
fn window(cx: &mut TestAppContext) {
    cx.add_window(|window, cx| {
        follow_appearance(window, cx);
        gpui::EmptyView
    });
    cx.run_until_parked();
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

fn assert_load_group(group: &Group, entered: &BTreeSet<u64>, expected_path: &Path) {
    assert_group(group, entered, "settings-load");
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected settings-load fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected settings-load path {display} (or {debug}), got {actual}"
    );
}

fn assert_watch_group(group: &Group, entered: &BTreeSet<u64>, app_init_id: u64) {
    assert_group(group, entered, "settings-watch");
    assert_eq!(
        group.parent,
        Some(app_init_id),
        "settings-watch must sit inside the group open when watch is called"
    );
    assert!(
        group.fields.is_empty(),
        "unexpected settings-watch fields: {:#?}",
        group.fields
    );
}

fn assert_register_group(group: &Group, entered: &BTreeSet<u64>, expected_section: &str) {
    assert_group(group, entered, "settings-register");
    assert_eq!(
        group.fields,
        BTreeMap::from([("section", expected_section.to_owned())])
    );
}

fn assert_apply_group(
    group: &Group,
    entered: &BTreeSet<u64>,
    parent: u64,
    mode: &str,
    theme: &str,
) {
    assert_group(group, entered, "theme-apply");
    assert_eq!(
        group.parent,
        Some(parent),
        "theme-apply must sit inside the group open when the theme is applied"
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

// A window's appearance report: appearance-change{appearance}, a group of its own.
fn assert_change_group(group: &Group, entered: &BTreeSet<u64>, appearance: &str) {
    assert_group(group, entered, "appearance-change");
    assert_eq!(
        group.parent, None,
        "appearance-change must be a group of its own"
    );
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["appearance"],
        "unexpected appearance-change fields: {:#?}",
        group.fields
    );
    assert_eq!(text(&group.fields["appearance"]), appearance);
}

fn assert_reload_group(
    group: &Group,
    entered: &BTreeSet<u64>,
    expected_path: &Path,
    watch_group: u64,
) {
    assert_group(group, entered, "settings-reload");
    assert_eq!(
        group.parent,
        Some(watch_group),
        "settings-reload must belong to settings-watch"
    );
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected settings-reload fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected settings-reload path {display} (or {debug}), got {actual}"
    );
}

fn assert_debug_event(
    event: &Event,
    expected_group: u64,
    expected_message: &str,
    expected_fields: &[(&'static str, &str)],
) {
    assert_eq!(event.group, Some(expected_group));
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some(expected_message));
    let expected: BTreeMap<&'static str, String> = expected_fields
        .iter()
        .map(|(k, v)| (*k, (*v).to_owned()))
        .collect();
    assert_eq!(event.fields, expected);
}

// One event alone inside theme-apply: `applied` or `unchanged` at DEBUG.
fn assert_apply_line<'a>(events: &[&'a Event], group: u64, message: &str) -> &'a Event {
    assert_eq!(
        events.len(),
        1,
        "expected `{message}` alone inside theme-apply, got {events:#?}"
    );
    let event = events[0];
    assert_eq!(event.group, Some(group));
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some(message));
    assert!(
        event.fields.is_empty(),
        "unexpected {message} fields: {:#?}",
        event.fields
    );
    event
}

fn groups_since(captured: &Captured, cursor: usize, expected: usize) -> &[Group] {
    let groups = &captured.groups[cursor..];
    assert_eq!(
        groups.len(),
        expected,
        "expected {expected} groups, got {groups:#?}"
    );
    groups
}

fn events_since(captured: &Captured, cursor: usize, expected: usize) -> Vec<&Event> {
    // The contract's `trace` lines are not asserted.
    let events: Vec<&Event> = captured.events[cursor..]
        .iter()
        .filter(|event| event.level != tracing::Level::TRACE)
        .collect();
    assert_eq!(
        events.len(),
        expected,
        "expected {expected} events, got {events:#?}"
    );
    events
}

fn events_in_group(captured: &Captured, cursor: usize, group: u64) -> Vec<&Event> {
    captured.events[cursor..]
        .iter()
        .filter(|event| event.group == Some(group))
        .collect()
}

fn diagnostics_since(captured: &Captured, cursor: usize, expected: usize) -> &[String] {
    let diagnostics = &captured.diagnostics[cursor..];
    assert_eq!(
        diagnostics.len(),
        expected,
        "expected {expected} diagnostics, got {diagnostics:#?}"
    );
    diagnostics
}

// The groups of one read that changed `theme`: settings-reload{path}, then
// theme-apply{mode, theme} and ui-font-apply{family, size}, inside settings-watch.
fn reload_then_apply<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    watch_id: u64,
    mode: &str,
    theme: &str,
) -> (&'a Group, &'a Group, &'a Group) {
    let [reload, apply, font] = &captured.groups[cursor..] else {
        panic!(
            "expected settings-reload, theme-apply, and ui-font-apply, got {:#?}",
            &captured.groups[cursor..]
        );
    };
    assert_reload_group(reload, &captured.entered_groups, path, watch_id);
    assert_apply_group(apply, &captured.entered_groups, watch_id, mode, theme);
    assert_group(font, &captured.entered_groups, "ui-font-apply");
    assert_eq!(font.parent, Some(watch_id));
    (reload, apply, font)
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn system_appearance_opens_groups_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let mut diagnostics_cursor = 0;

        // 1. Startup while the OS is dark and no file: system mode applies the
        //    dark theme. Groups in order: settings-init, settings-load,
        //    settings-watch, ui-init, settings-register{section=theme},
        //    theme-apply{mode=system, theme=One Dark},
        //    ui-font-apply{family, size}.
        let path = temp.path().join("settings.json");
        let app_init = cx.update(|cx| startup(cx, &path, theme::Appearance::Dark));
        cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
        let (watch_id, mut groups_cursor, mut events_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let [
                init_g,
                load_g,
                watch_g,
                ui_init_g,
                register_g,
                apply_g,
                font_g,
            ] = &captured.groups[..]
            else {
                panic!("expected 7 startup groups, got {:#?}", captured.groups);
            };
            assert_group(init_g, &captured.entered_groups, "settings-init");
            assert!(
                init_g.fields.is_empty(),
                "unexpected settings-init fields: {:#?}",
                init_g.fields
            );
            assert_load_group(load_g, &captured.entered_groups, &path);
            assert_watch_group(watch_g, &captured.entered_groups, app_init);
            assert_group(ui_init_g, &captured.entered_groups, "ui-init");
            assert!(
                ui_init_g.fields.is_empty(),
                "unexpected ui-init fields: {:#?}",
                ui_init_g.fields
            );
            assert_register_group(register_g, &captured.entered_groups, "theme");
            assert_apply_group(
                apply_g,
                &captured.entered_groups,
                app_init,
                "system",
                "One Dark",
            );
            assert_group(font_g, &captured.entered_groups, "ui-font-apply");
            assert_eq!(font_g.parent, Some(app_init));
            let started = events_in_group(&captured, 0, watch_g.id);
            assert_eq!(
                started.len(),
                1,
                "expected `started` alone inside settings-watch, got {started:#?}"
            );
            assert_debug_event(started[0], watch_g.id, "started", &[]);
            assert_apply_line(
                &events_in_group(&captured, 0, apply_g.id),
                apply_g.id,
                "applied",
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            diagnostics_cursor = captured.diagnostics.len();
            (watch_g.id, captured.groups.len(), captured.events.len())
        };

        // 2. A window reports the OS's appearance — the test platform's light —
        //    which differs from the recorded dark: inside
        //    appearance-change{appearance=light} the theme is applied again,
        //    theme-apply{mode=system, theme=One Light} with `applied`.
        window(cx);
        cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let [change, apply_g] = &captured.groups[groups_cursor..] else {
                panic!(
                    "expected appearance-change then theme-apply, got {:#?}",
                    &captured.groups[groups_cursor..]
                );
            };
            assert_change_group(change, &captured.entered_groups, "light");
            assert_apply_group(
                apply_g,
                &captured.entered_groups,
                change.id,
                "system",
                "One Light",
            );
            let events = events_since(&captured, events_cursor, 1);
            assert_apply_line(&events, apply_g.id, "applied");
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 3. The same appearance is reported again: `unchanged` inside
        //    appearance-change{appearance=light}, and nothing else.
        window(cx);
        cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let change = &groups_since(&captured, groups_cursor, 1)[0];
            assert_change_group(change, &captured.entered_groups, "light");
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], change.id, "unchanged", &[]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 4. The mode becomes dark and One Dark applies, then a window's report
        //    of light — a change against the recorded dark — is recorded and
        //    `mode is not system` is logged: the theme stays.
        fs::write(&path, r#"{"theme": {"mode": "dark"}}"#).unwrap();
        tick(cx);
        cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (reload, apply_g, font_g) = reload_then_apply(
                &captured,
                groups_cursor,
                &path,
                watch_id,
                "dark",
                "One Dark",
            );
            let events = events_since(&captured, events_cursor, 4);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "theme")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            let applied = assert_apply_line(&events[2..3], apply_g.id, "applied");
            assert_debug_event(events[3], font_g.id, "unchanged", &[]);
            assert!(
                events[1].seq < applied.seq,
                "the apply must run after the read that changed the section"
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        cx.update(|cx| {
            theme::SystemAppearance::global_mut(cx).0 = theme::Appearance::Dark;
        });
        window(cx);
        cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
        {
            let captured = CAPTURED.lock().unwrap();
            let change = &groups_since(&captured, groups_cursor, 1)[0];
            assert_change_group(change, &captured.entered_groups, "light");
            let events = events_since(&captured, events_cursor, 1);
            assert_debug_event(events[0], change.id, "mode is not system", &[]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 5. The section's mode becomes system: theme-apply applies the theme
        //    of the current appearance — light, as last recorded — with
        //    `applied`, inside settings-watch.
        fs::write(&path, r#"{"theme": {"mode": "system"}}"#).unwrap();
        tick(cx);
        cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (reload, apply_g, font_g) = reload_then_apply(
                &captured,
                groups_cursor,
                &path,
                watch_id,
                "system",
                "One Light",
            );
            let events = events_since(&captured, events_cursor, 4);
            assert_debug_event(events[0], reload.id, "changed", &[("section", "theme")]);
            assert_debug_event(events[1], reload.id, "reloaded", &[("changed", "1")]);
            let applied = assert_apply_line(&events[2..3], apply_g.id, "applied");
            assert_debug_event(events[3], font_g.id, "unchanged", &[]);
            assert!(
                events[1].seq < applied.seq,
                "the apply must run after the read that changed the section"
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
