use gpui::TestAppContext;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_ui::{choose_theme, theme_list};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-ui-theme-list-logging-{}-{}-{nanos:x}",
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
    group: Option<u64>,
    level: tracing::Level,
    message: Option<String>,
    fields: BTreeMap<&'static str, String>,
}

#[derive(Debug)]
struct Diagnostic {
    level: log::Level,
    group: Option<u64>,
    message: String,
}

struct Captured {
    next_id: u64,
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
            spans: BTreeMap::new(),
            entered: Vec::new(),
            entered_groups: BTreeSet::new(),
            groups: Vec::new(),
            events: Vec::new(),
            diagnostics: Vec::new(),
        }
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
        let group = captured.current_group(record.metadata().target());
        captured.diagnostics.push(Diagnostic {
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
        let group = captured.current_group(metadata.target());
        captured.events.push(Event {
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

// The store, the file, the UI, then the theme setting, inside the group that
// surrounds them, as `app-init` does in the application.
fn startup(cx: &mut gpui::App, settings: &Path, content: &str, appearance: theme::Appearance) {
    fs::write(settings, content).unwrap();
    let app_init = tracing::info_span!("app-init");
    let _group = app_init.entered();
    yard_settings::init(cx);
    yard_settings::load(cx, settings.to_path_buf());
    yard_ui::init(cx);
    theme::SystemAppearance::global_mut(cx).0 = appearance;
    yard_ui::apply_theme_setting(cx);
}

fn cursors(captured: &Captured) -> (usize, usize, usize) {
    (
        captured.groups.len(),
        captured.events.len(),
        captured.diagnostics.len(),
    )
}

fn assert_entered(group: &Group, entered: &BTreeSet<u64>) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
}

// The only `theme-choose` group since `cursor`, entered and named `name`.
fn choose_group<'a>(captured: &'a Captured, cursor: usize, name: &str) -> &'a Group {
    let choose: Vec<&Group> = captured.groups[cursor..]
        .iter()
        .filter(|group| group.name == "theme-choose")
        .collect();
    assert_eq!(
        choose.len(),
        1,
        "expected one theme-choose group, got {choose:#?}"
    );
    let choose = choose[0];
    assert_entered(choose, &captured.entered_groups);
    let fields: BTreeMap<&'static str, String> = choose
        .fields
        .iter()
        .map(|(key, value)| (*key, text(value).to_owned()))
        .collect();
    assert_eq!(
        fields,
        BTreeMap::from([("name", name.to_owned())]),
        "theme-choose must hold only `name`"
    );
    choose
}

// Every settings-write group since `cursor` must sit inside `choose_id` and
// name the `theme` section; returns them with the field each one writes.
fn writes_inside(captured: &Captured, cursor: usize, choose_id: u64) -> Vec<(&Group, String)> {
    let mut writes = Vec::new();
    for group in &captured.groups[cursor..] {
        if group.name != "settings-write" {
            continue;
        }
        assert_entered(group, &captured.entered_groups);
        assert_eq!(
            group.parent,
            Some(choose_id),
            "settings-write must sit inside theme-choose"
        );
        assert_eq!(
            group.fields.keys().copied().collect::<Vec<_>>(),
            ["field", "section"],
            "unexpected settings-write fields: {:#?}",
            group.fields
        );
        assert_eq!(text(&group.fields["section"]), "theme");
        writes.push((group, text(&group.fields["field"]).to_owned()));
    }
    writes
}

// The settings-reload groups since `cursor`: each one inside a settings-write
// of the choice, naming the settings file.
fn assert_reloads(
    captured: &Captured,
    cursor: usize,
    writes: &[&Group],
    expected: usize,
    path: &Path,
) {
    let write_ids: BTreeSet<u64> = writes.iter().map(|group| group.id).collect();
    let reloads: Vec<&Group> = captured.groups[cursor..]
        .iter()
        .filter(|group| group.name == "settings-reload")
        .collect();
    assert_eq!(
        reloads.len(),
        expected,
        "expected {expected} settings-reload groups, got {reloads:#?}"
    );
    for reload in reloads {
        assert_entered(reload, &captured.entered_groups);
        assert!(
            reload
                .parent
                .is_some_and(|parent| write_ids.contains(&parent)),
            "settings-reload must sit inside a settings-write of the choice"
        );
        assert_eq!(
            reload.fields.keys().copied().collect::<Vec<_>>(),
            ["path"],
            "unexpected settings-reload fields: {:#?}",
            reload.fields
        );
        let actual = &reload.fields["path"];
        let display = path.display().to_string();
        let debug = format!("{path:?}");
        assert!(
            actual == &display || actual == &debug,
            "expected settings-reload path {display} (or {debug}), got {actual}"
        );
    }
}

// `theme chosen` alone inside theme-choose, at DEBUG, with `field` and `mode`.
fn assert_chosen(captured: &Captured, cursor: usize, choose_id: u64, field: &str, mode: &str) {
    let events: Vec<&Event> = captured.events[cursor..]
        .iter()
        .filter(|event| event.group == Some(choose_id))
        .collect();
    assert_eq!(
        events.len(),
        1,
        "expected `theme chosen` alone inside theme-choose, got {events:#?}"
    );
    let event = events[0];
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some("theme chosen"));
    let fields: BTreeMap<&'static str, String> = event
        .fields
        .iter()
        .map(|(key, value)| (*key, text(value).to_owned()))
        .collect();
    assert_eq!(
        fields,
        BTreeMap::from([("field", field.to_owned()), ("mode", mode.to_owned()),])
    );
}

// No group since `cursor` is one the contract does not name: the choice, its
// writes and their reloads, or the apply the write reaches at once.
fn assert_only_choice_groups(captured: &Captured, cursor: usize) {
    for group in &captured.groups[cursor..] {
        assert!(
            matches!(
                group.name,
                "theme-choose"
                    | "settings-write"
                    | "settings-reload"
                    | "theme-apply"
                    | "ui-font-apply"
                    | "density-apply"
            ),
            "unexpected group: {group:#?}"
        );
    }
}

fn events_since(captured: &Captured, cursor: usize, expected: usize) {
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
fn choose_theme_opens_theme_choose_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let dark = temp.path().join("dark.json");
        cx.update(|cx| {
            startup(
                cx,
                &dark,
                r#"{"theme": {"mode": "dark"}}"#,
                theme::Appearance::Light,
            )
        });
        let (mut groups_cursor, mut events_cursor, mut diagnostics_cursor) =
            cursors(&CAPTURED.lock().unwrap());

        // 1. Reading the list logs nothing, however often it is read: a
        //    component may read it on every frame.
        cx.read(|cx| assert_eq!(theme_list(cx).len(), 2));
        cx.read(|cx| assert_eq!(theme_list(cx).len(), 2));
        {
            let captured = CAPTURED.lock().unwrap();
            assert_eq!(captured.groups.len(), groups_cursor);
            assert_eq!(captured.events.len(), events_cursor);
            assert_eq!(captured.diagnostics.len(), diagnostics_cursor);
        }

        // 2. Choosing One Light opens theme-choose{name=One Light} around the
        //    writes of `light` and `mode`, each with its settings-reload
        //    inside it, and `theme chosen` inside the choice with
        //    field=light and mode=light.
        cx.update(|cx| choose_theme(cx, "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            assert_only_choice_groups(&captured, groups_cursor);
            let choose = choose_group(&captured, groups_cursor, "One Light");
            let writes = writes_inside(&captured, groups_cursor, choose.id);
            let fields: BTreeSet<String> = writes.iter().map(|(_, field)| field.clone()).collect();
            assert_eq!(
                fields,
                BTreeSet::from(["light".to_owned(), "mode".to_owned()]),
                "the choice must write `light` and `mode`: {writes:#?}"
            );
            assert_reloads(
                &captured,
                groups_cursor,
                &writes.iter().map(|(group, _)| *group).collect::<Vec<_>>(),
                2,
                &dark,
            );
            assert_chosen(&captured, events_cursor, choose.id, "light", "light");
            assert!(
                captured.groups[groups_cursor..]
                    .iter()
                    .any(|group| group.name == "theme-apply"),
                "the write must reach the theme setting at once"
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            (groups_cursor, events_cursor, diagnostics_cursor) = cursors(&captured);
        }

        // 3. A name the list never offered: theme-choose{name=Solarized} alone,
        //    the ERROR inside it, and nothing written.
        cx.update(|cx| choose_theme(cx, "Solarized"));
        {
            let captured = CAPTURED.lock().unwrap();
            let choose = choose_group(&captured, groups_cursor, "Solarized");
            assert_eq!(
                captured.groups.len() - groups_cursor,
                1,
                "nothing but theme-choose may open"
            );
            events_since(&captured, events_cursor, 0);
            let errors = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_eq!(errors[0].level, log::Level::Error);
            assert_eq!(errors[0].group, Some(choose.id));
            assert_eq!(errors[0].message, "theme Solarized is not registered");
        }

        // 4. `mode` is system and the system is dark: choosing the dark theme
        //    writes `dark` and keeps `mode` system, so `theme chosen` logs
        //    field=dark and mode=system.
        let system = temp.path().join("system.json");
        let app = cx.new_app();
        app.update(|cx| {
            startup(
                cx,
                &system,
                r#"{"theme": {"mode": "system"}}"#,
                theme::Appearance::Dark,
            )
        });
        (groups_cursor, events_cursor, diagnostics_cursor) = cursors(&CAPTURED.lock().unwrap());
        app.update(|cx| choose_theme(cx, "One Dark"));
        {
            let captured = CAPTURED.lock().unwrap();
            assert_only_choice_groups(&captured, groups_cursor);
            let choose = choose_group(&captured, groups_cursor, "One Dark");
            let writes = writes_inside(&captured, groups_cursor, choose.id);
            assert!(
                writes.iter().any(|(_, field)| field.as_str() == "dark"),
                "the dark theme must be written to `dark`: {writes:#?}"
            );
            assert!(
                writes
                    .iter()
                    .all(|(_, field)| matches!(field.as_str(), "dark" | "mode")),
                "only `dark` and `mode` may be written: {writes:#?}"
            );
            assert_reloads(
                &captured,
                groups_cursor,
                &writes.iter().map(|(group, _)| *group).collect::<Vec<_>>(),
                1,
                &system,
            );
            assert_chosen(&captured, events_cursor, choose.id, "dark", "system");
            assert!(
                captured.groups[groups_cursor..]
                    .iter()
                    .any(|group| group.name == "theme-apply"),
                "the write must reach the theme setting at once"
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
