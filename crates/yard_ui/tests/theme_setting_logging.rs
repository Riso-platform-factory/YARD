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

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-ui-theme-setting-{}-{}-{nanos:x}",
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

// The startup's order: the store, the file, the UI, then the theme setting,
// inside the group that surrounds them, as `app-init` does in the application.
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

// The seven groups of one startup, in order: settings-init, settings-load,
// ui-init, settings-register{section=theme}, theme-apply{mode, theme},
// ui-font-apply{family, size}, density-apply{density}.
// Returns the settings-register and theme-apply groups.
fn scenario_groups<'a>(
    captured: &'a Captured,
    cursor: usize,
    path: &Path,
    app_init_id: u64,
    mode: &str,
    theme: &str,
) -> (&'a Group, &'a Group) {
    let [
        init_g,
        load_g,
        ui_init_g,
        register_g,
        apply_g,
        font_g,
        density_g,
    ] = &captured.groups[cursor..]
    else {
        panic!("expected 7 groups, got {:#?}", &captured.groups[cursor..]);
    };
    assert_group(init_g, &captured.entered_groups, "settings-init");
    assert!(
        init_g.fields.is_empty(),
        "unexpected settings-init fields: {:#?}",
        init_g.fields
    );
    assert_load_group(load_g, &captured.entered_groups, path);
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
    assert_apply_group(apply_g, &captured.entered_groups, app_init_id, mode, theme);
    assert_group(font_g, &captured.entered_groups, "ui-font-apply");
    assert_eq!(font_g.parent, Some(app_init_id));
    assert_group(density_g, &captured.entered_groups, "density-apply");
    assert_eq!(density_g.parent, Some(app_init_id));
    (register_g, apply_g)
}

fn events_in_group(captured: &Captured, cursor: usize, group: u64) -> Vec<&Event> {
    captured.events[cursor..]
        .iter()
        .filter(|event| event.group == Some(group))
        .collect()
}

fn assert_applied<'a>(events: &[&'a Event]) -> &'a Event {
    assert_eq!(
        events.len(),
        1,
        "expected `applied` alone inside theme-apply, got {events:#?}"
    );
    let applied = events[0];
    assert_eq!(applied.level, tracing::Level::DEBUG);
    assert_eq!(applied.message.as_deref(), Some("applied"));
    assert!(
        applied.fields.is_empty(),
        "unexpected applied fields: {:#?}",
        applied.fields
    );
    applied
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
fn theme_setting_opens_theme_apply_and_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let mut groups_cursor = 0;
        let mut events_cursor = 0;
        let mut diagnostics_cursor = 0;

        // 1. No file: the defaults hold. theme-apply{mode=system, theme=One Light},
        //    `applied` at DEBUG.
        let missing = temp.path().join("missing.json");
        let app_init = cx.update(|cx| startup(cx, &missing, None));
        cx.read(|cx| assert_eq!(yard_ui::active_theme(cx), "One Light"));
        assert!(!missing.exists());
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, apply_g) = scenario_groups(
                &captured,
                groups_cursor,
                &missing,
                app_init,
                "system",
                "One Light",
            );
            assert_applied(&events_in_group(&captured, events_cursor, apply_g.id));
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 2. `mode` chooses a registered theme:
        //    theme-apply{mode=light, theme=One Light}, `applied`.
        let light = temp.path().join("light.json");
        let app = cx.new_app();
        let app_init =
            app.update(|cx| startup(cx, &light, Some(r#"{"theme": {"mode": "light"}}"#)));
        app.read(|cx| assert_eq!(yard_ui::active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, apply_g) = scenario_groups(
                &captured,
                groups_cursor,
                &light,
                app_init,
                "light",
                "One Light",
            );
            assert_applied(&events_in_group(&captured, events_cursor, apply_g.id));
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 3. `mode=dark` uses the name in `dark`:
        //    theme-apply{mode=dark, theme=One Light}, `applied`.
        let named = temp.path().join("named.json");
        let app = cx.new_app();
        let app_init = app.update(|cx| {
            startup(
                cx,
                &named,
                Some(r#"{"theme": {"mode": "dark", "dark": "One Light"}}"#),
            )
        });
        app.read(|cx| assert_eq!(yard_ui::active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, apply_g) = scenario_groups(
                &captured,
                groups_cursor,
                &named,
                app_init,
                "dark",
                "One Light",
            );
            assert_applied(&events_in_group(&captured, events_cursor, apply_g.id));
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 4. The chosen name is not registered: One Dark stays, and the WARN
        //    `theme Solarized is not registered` is logged inside theme-apply.
        let solarized = temp.path().join("solarized.json");
        let app = cx.new_app();
        let app_init = app.update(|cx| {
            startup(
                cx,
                &solarized,
                Some(r#"{"theme": {"mode": "light", "light": "Solarized"}}"#),
            )
        });
        app.read(|cx| assert_eq!(yard_ui::active_theme(cx), "One Dark"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (_, apply_g) = scenario_groups(
                &captured,
                groups_cursor,
                &solarized,
                app_init,
                "light",
                "Solarized",
            );
            assert!(
                events_in_group(&captured, events_cursor, apply_g.id).is_empty(),
                "nothing is applied when the theme stays"
            );
            let warn = &diagnostics_since(&captured, diagnostics_cursor, 1)[0];
            assert_eq!(warn.level, log::Level::Warn);
            assert_eq!(warn.group, Some(apply_g.id));
            assert_eq!(warn.message, "theme Solarized is not registered");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // 5. A `mode` that is not system, dark, or light makes the section unreadable:
        //    the settings engine's WARN inside settings-register{section=theme},
        //    then `applied` for the defaults in theme-apply{mode=system, theme=One Light}.
        let dim = temp.path().join("dim.json");
        let app = cx.new_app();
        let app_init = app.update(|cx| startup(cx, &dim, Some(r#"{"theme": {"mode": "dim"}}"#)));
        app.read(|cx| assert_eq!(yard_ui::active_theme(cx), "One Light"));
        {
            let captured = CAPTURED.lock().unwrap();
            let (register_g, apply_g) = scenario_groups(
                &captured,
                groups_cursor,
                &dim,
                app_init,
                "system",
                "One Light",
            );
            let applied = assert_applied(&events_in_group(&captured, events_cursor, apply_g.id));
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
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
}
