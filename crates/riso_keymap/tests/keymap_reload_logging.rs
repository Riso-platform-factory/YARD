#![cfg(feature = "dev")]

use gpui::TestAppContext;
use riso_keymap::{dev::watch_user_keymap, load_user_keymap};
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

gpui::actions!(keymap_test, [Ping, Pong]);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-keymap-reload-logging-{}-{}-{nanos:x}",
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

    fn current_keymap_group(&self) -> Option<u64> {
        self.entered
            .iter()
            .rev()
            .copied()
            .find(|id| self.spans[id].starts_with("riso_keymap"))
    }
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_keymap")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_keymap_group();
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
        let target = metadata.target();
        let mut captured = CAPTURED.lock().unwrap();
        let id = captured.next_id;
        captured.next_id += 1;
        if target.starts_with("riso_keymap") {
            let parent = captured.current_keymap_group();
            captured.groups.push(Group {
                id,
                parent,
                level: *metadata.level(),
                name: metadata.name(),
                fields: fields.values,
            });
        }
        captured.spans.insert(id, target);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("riso_keymap") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = CAPTURED.lock().unwrap();
        let seq = captured.next_sequence();
        let group = captured.current_keymap_group();
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
        if captured.spans[&id].starts_with("riso_keymap") {
            captured.entered_groups.insert(id);
        }
        captured.entered.push(id);
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(CAPTURED.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

/// Whether `group` is `ancestor` or a group nested in it through `parent`.
fn is_inside(captured: &Captured, mut group: Option<u64>, ancestor: u64) -> bool {
    while let Some(id) = group {
        if id == ancestor {
            return true;
        }
        group = captured
            .groups
            .iter()
            .find(|candidate| candidate.id == id)
            .and_then(|candidate| candidate.parent);
    }
    false
}

fn assert_path(group: &Group, expected_path: &Path) {
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected {} fields: {:#?}",
        group.name,
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected {} path {display} (or {debug}), got {actual}",
        group.name
    );
}

fn assert_watch_group(group: &Group, entered: &BTreeSet<u64>, expected_path: &Path) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "keymap-watch");
    assert_eq!(
        group.parent, None,
        "keymap-watch must not belong to another keymap group"
    );
    assert_path(group, expected_path);
}

fn assert_reload_group(group: &Group, entered: &BTreeSet<u64>, expected_path: &Path, watch: u64) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "keymap-reload");
    assert_eq!(
        group.parent,
        Some(watch),
        "keymap-reload must be opened inside keymap-watch"
    );
    assert_path(group, expected_path);
}

/// Asserts that the groups opened since `cursor` are one `keymap-reload` inside
/// `keymap-watch`, plus whatever `user-keymap` opens inside it to bind the
/// file. Returns the reload group's id.
fn reload_since(captured: &Captured, cursor: usize, expected_path: &Path, watch: u64) -> u64 {
    let groups = &captured.groups[cursor..];
    assert!(
        !groups.is_empty(),
        "expected `keymap-reload` inside `keymap-watch`, got nothing"
    );
    let reload = &groups[0];
    assert_reload_group(reload, &captured.entered_groups, expected_path, watch);
    for extra in &groups[1..] {
        assert!(
            is_inside(captured, extra.parent, reload.id),
            "a group opened outside `keymap-reload`: {extra:#?}"
        );
        assert_ne!(
            extra.name, "keymap-reload",
            "a second `keymap-reload` group: {extra:#?}"
        );
    }
    reload.id
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

/// A line of `user-keymap` may sit in `keymap-reload` or in a group inside it.
fn assert_debug_event_inside(
    captured: &Captured,
    event: &Event,
    ancestor: u64,
    expected_message: &str,
    expected_fields: &[(&'static str, &str)],
) {
    assert!(
        is_inside(captured, event.group, ancestor),
        "event outside `keymap-reload`: {event:#?}"
    );
    assert_eq!(event.level, tracing::Level::DEBUG);
    assert_eq!(event.message.as_deref(), Some(expected_message));
    let expected: BTreeMap<&'static str, String> = expected_fields
        .iter()
        .map(|(k, v)| (*k, (*v).to_owned()))
        .collect();
    assert_eq!(event.fields, expected);
}

fn assert_warn_inside(
    captured: &Captured,
    diagnostic: &Diagnostic,
    ancestor: u64,
    expected_prefix: &str,
) {
    assert_eq!(diagnostic.level, log::Level::Warn);
    assert!(
        is_inside(captured, diagnostic.group, ancestor),
        "WARN outside `keymap-reload`: {diagnostic:#?}"
    );
    let Some(reason) = diagnostic.message.strip_prefix(expected_prefix) else {
        panic!("unexpected diagnostic message: {}", diagnostic.message);
    };
    assert!(
        !reason.trim().is_empty(),
        "no reason after {expected_prefix:?} in {:?}",
        diagnostic.message
    );
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

fn diagnostics_since(captured: &Captured, cursor: usize, expected: usize) -> &[Diagnostic] {
    let diagnostics = &captured.diagnostics[cursor..];
    assert_eq!(
        diagnostics.len(),
        expected,
        "expected {expected} diagnostics, got {diagnostics:#?}"
    );
    diagnostics
}

/// A check that finds nothing new opens no group and logs nothing.
fn assert_nothing_new(captured: &Captured, cursors: (usize, usize, usize)) {
    groups_since(captured, cursors.0, 0);
    events_since(captured, cursors.1, 0);
    diagnostics_since(captured, cursors.2, 0);
}

// Alone in this binary: installs the process-wide diagnostic logger and
// captures every callsite that reaches the keymap's lines.
#[gpui::test]
fn watching_and_reloading_log_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        let path = temp.path().join("keymap.json");
        fs::write(
            &path,
            r#"[{"context": "Shell", "bindings": {"ctrl-p": "keymap_test::Pong"}}]"#,
        )
        .unwrap();
        cx.update(|cx| {
            cx.bind_keys(vec![
                gpui::KeyBinding::new("ctrl-a", Ping, Some("Shell")),
                gpui::KeyBinding::new("ctrl-b", Pong, Some("Shell")),
            ]);
            load_user_keymap(cx, &path);
            watch_user_keymap(cx, path.clone());
        });

        // The watching starts: `keymap-watch{path}` opens — it lasts while the
        // application runs — and logs `started` at DEBUG.
        let (watch_id, mut groups_cursor, mut events_cursor, mut diagnostics_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let [load, watch] = &captured.groups[..] else {
                panic!("expected 2 groups, got {:#?}", captured.groups);
            };
            assert_eq!(load.name, "keymap-load");
            assert_watch_group(watch, &captured.entered_groups, &path);
            // `bound` and `user keys bound` belong to the first load's group;
            // `started` opens the watch's lines.
            let events = events_since(&captured, 0, 3);
            assert_debug_event(events[2], watch.id, "started", &[]);
            assert!(
                captured.diagnostics.is_empty(),
                "expected no diagnostics, got {:#?}",
                captured.diagnostics
            );
            (
                watch.id,
                captured.groups.len(),
                captured.events.len(),
                captured.diagnostics.len(),
            )
        };

        // A check that finds nothing new opens no group and logs nothing.
        for _ in 0..2 {
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
        }
        {
            let captured = CAPTURED.lock().unwrap();
            assert_nothing_new(
                &captured,
                (groups_cursor, events_cursor, diagnostics_cursor),
            );
        }

        // The file changed: inside `keymap-watch`, `keymap-reload{path}` logs
        // `defaults bound` with `count`, then the lines of `user-keymap` for
        // the file: `bound`, each `WARN`, and `user keys bound`.
        fs::write(
            &path,
            r#"[
                {"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping"}},
                "not an object"
            ]"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = reload_since(&captured, groups_cursor, &path, watch_id);
            let events = events_since(&captured, events_cursor, 3);
            assert_debug_event(events[0], reload, "defaults bound", &[("count", "2")]);
            assert_debug_event_inside(
                &captured,
                events[1],
                reload,
                "bound",
                &[
                    ("keys", "cmd-shift-z"),
                    ("action", "keymap_test::Ping"),
                    ("context", "Shell"),
                ],
            );
            assert_debug_event_inside(
                &captured,
                events[2],
                reload,
                "user keys bound",
                &[("count", "1"), ("skipped", "1")],
            );
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_warn_inside(
                &captured,
                &diagnostics[0],
                reload,
                "keymap.json: section 1: ",
            );
            assert!(
                diagnostics[0].seq < events[2].seq,
                "a WARN was logged after `user keys bound`"
            );
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // The same file again is nothing new.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            assert_nothing_new(
                &captured,
                (groups_cursor, events_cursor, diagnostics_cursor),
            );
        }

        // The file deleted: `defaults bound`, then `no keymap file`.
        fs::remove_file(&path).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = reload_since(&captured, groups_cursor, &path, watch_id);
            let events = events_since(&captured, events_cursor, 2);
            assert_debug_event(events[0], reload, "defaults bound", &[("count", "2")]);
            assert_debug_event_inside(&captured, events[1], reload, "no keymap file", &[]);
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // While the file stays deleted, a check finds nothing new.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            assert_nothing_new(
                &captured,
                (groups_cursor, events_cursor, diagnostics_cursor),
            );
        }

        // The file created again binds its keys.
        fs::write(
            &path,
            r#"[{"context": "Shell", "bindings": {"cmd-alt-q": "keymap_test::Pong"}}]"#,
        )
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = reload_since(&captured, groups_cursor, &path, watch_id);
            let events = events_since(&captured, events_cursor, 3);
            assert_debug_event(events[0], reload, "defaults bound", &[("count", "2")]);
            assert_debug_event_inside(
                &captured,
                events[1],
                reload,
                "bound",
                &[
                    ("keys", "cmd-alt-q"),
                    ("action", "keymap_test::Pong"),
                    ("context", "Shell"),
                ],
            );
            assert_debug_event_inside(
                &captured,
                events[2],
                reload,
                "user keys bound",
                &[("count", "1"), ("skipped", "0")],
            );
            diagnostics_since(&captured, diagnostics_cursor, 0);
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A file that is not valid JSON: `defaults bound`, then its `WARN`. As
        // `user-keymap` leaves it, whether such a file still ends in `user
        // keys bound` is for the implementation to say; if it logs one, it
        // must be complete.
        fs::write(&path, "not json").unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            let reload = reload_since(&captured, groups_cursor, &path, watch_id);
            let events: Vec<&Event> = captured.events[events_cursor..]
                .iter()
                .filter(|event| event.level != tracing::Level::TRACE)
                .collect();
            assert!(
                events.len() == 1 || events.len() == 2,
                "expected `defaults bound` and at most `user keys bound`, got {events:#?}"
            );
            assert_debug_event(events[0], reload, "defaults bound", &[("count", "2")]);
            if let Some(summary) = events.get(1) {
                assert!(
                    is_inside(&captured, summary.group, reload),
                    "event outside `keymap-reload`: {summary:#?}"
                );
                assert_eq!(summary.level, tracing::Level::DEBUG);
                assert_eq!(summary.message.as_deref(), Some("user keys bound"));
                assert_eq!(
                    summary
                        .fields
                        .keys()
                        .copied()
                        .collect::<Vec<_>>()
                        .as_slice(),
                    ["count", "skipped"].as_slice(),
                    "unexpected fields in {summary:#?}"
                );
                assert_eq!(summary.fields["count"], "0");
            }
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_warn_inside(&captured, &diagnostics[0], reload, "keymap.json:");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // The same broken content is not reported again.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        {
            let captured = CAPTURED.lock().unwrap();
            assert_nothing_new(
                &captured,
                (groups_cursor, events_cursor, diagnostics_cursor),
            );
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
    assert!(
        !captured
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.level == log::Level::Error),
        "unexpected error in {:#?}",
        captured.diagnostics
    );
}
