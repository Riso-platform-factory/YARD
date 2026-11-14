use gpui::TestAppContext;
use riso_keymap::load_user_keymap;
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

gpui::actions!(keymap_test, [Ping, Pong]);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-keymap-logging-{}-{}-{nanos:x}",
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
            captured.groups.push(Group {
                id,
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

fn assert_load_group(group: &Group, entered: &BTreeSet<u64>, expected_path: &Path) {
    assert!(
        entered.contains(&group.id),
        "group {} was opened but never entered",
        group.id
    );
    assert_eq!(group.level, tracing::Level::INFO);
    assert_eq!(group.name, "keymap-load");
    assert_eq!(
        group.fields.keys().copied().collect::<Vec<_>>(),
        ["path"],
        "unexpected keymap-load fields: {:#?}",
        group.fields
    );
    let actual = &group.fields["path"];
    let display = expected_path.display().to_string();
    let debug = format!("{expected_path:?}");
    assert!(
        actual == &display || actual == &debug,
        "expected keymap-load path {display} (or {debug}), got {actual}"
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

fn assert_warn(diagnostic: &Diagnostic, expected_group: u64, expected_prefix: &str) {
    assert_eq!(diagnostic.level, log::Level::Warn);
    assert_eq!(diagnostic.group, Some(expected_group));
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

// Alone in this binary: installs the process-wide diagnostic logger and
// captures every callsite that reaches the keymap's lines.
#[gpui::test]
fn load_user_keymap_logs_every_contract_line(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let temp = TempDir::new();

    tracing::subscriber::with_default(Trace, || {
        // No file: `keymap-load{path}` opens, `no keymap file` inside, and
        // nothing more.
        let missing = temp.path().join("missing/keymap.json");
        assert_eq!(cx.update(|cx| load_user_keymap(cx, &missing)), 0);
        let (mut groups_cursor, mut events_cursor, mut diagnostics_cursor) = {
            let captured = CAPTURED.lock().unwrap();
            let groups = groups_since(&captured, 0, 1);
            let group = groups[0].id;
            assert_load_group(&groups[0], &captured.entered_groups, &missing);
            let events = events_since(&captured, 0, 1);
            assert_debug_event(events[0], group, "no keymap file", &[]);
            // "and nothing more": no other event at all, no diagnostic.
            assert_eq!(captured.events.len(), 1);
            diagnostics_since(&captured, 0, 0);
            (
                captured.groups.len(),
                captured.events.len(),
                captured.diagnostics.len(),
            )
        };

        // A file: a `bound` line per bound binding (context empty when none), a
        // WARN naming each entry that cannot be read, and `user keys bound`
        // with count and skipped at the end.
        let path = temp.path().join("keymap.json");
        fs::write(
            &path,
            r#"[
                {"context": "Shell", "bindings": {"cmd-shift-z": "keymap_test::Ping", "shift-escape": null}},
                {"bindings": {"cmd-alt-q": ["zed::Unbind", "keymap_test::Ping"]}},
                "not an object",
                {"context": "Shell &&", "bindings": {"ctrl-z": "keymap_test::Pong"}},
                {"bindings": {"cmd-+-x": "keymap_test::Ping", "ctrl-n": "keymap_test::Nope", "ctrl-a": "keymap_test::Pong"}}
            ]"#,
        )
        .unwrap();
        assert_eq!(cx.update(|cx| load_user_keymap(cx, &path)), 4);
        // The counted bindings are in the keymap.
        let (pings_bound, pongs_bound) = cx.update(|cx| {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            (
                keymap.bindings_for_action(&Ping).count(),
                keymap.bindings_for_action(&Pong).count(),
            )
        });
        assert_eq!((pings_bound, pongs_bound), (1, 1));
        {
            let captured = CAPTURED.lock().unwrap();
            let groups = groups_since(&captured, groups_cursor, 1);
            let group = groups[0].id;
            assert_load_group(&groups[0], &captured.entered_groups, &path);

            let events = events_since(&captured, events_cursor, 5);
            let bound_lines = &events[..4];
            let summary = events[4];
            for (keys, action, context) in [
                ("cmd-shift-z", "keymap_test::Ping", "Shell"),
                ("shift-escape", "zed::NoAction", "Shell"),
                ("cmd-alt-q", "zed::Unbind", ""),
                ("ctrl-a", "keymap_test::Pong", ""),
            ] {
                let matching = bound_lines
                    .iter()
                    .filter(|event| {
                        event.group == Some(group)
                            && event.level == tracing::Level::DEBUG
                            && event.message.as_deref() == Some("bound")
                            && event.fields
                                == BTreeMap::from([
                                    ("keys", keys.to_owned()),
                                    ("action", action.to_owned()),
                                    ("context", context.to_owned()),
                                ])
                    })
                    .count();
                assert_eq!(
                    matching, 1,
                    "one `bound` line with keys={keys} action={action} context={context:?} in {bound_lines:#?}"
                );
            }
            assert_debug_event(
                summary,
                group,
                "user keys bound",
                &[("count", "4"), ("skipped", "4")],
            );

            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 4);
            let whats = ["section 2", "section 3", "cmd-+-x", "ctrl-n"];
            for (diagnostic, what) in diagnostics.iter().zip(whats) {
                assert_warn(diagnostic, group, &format!("keymap.json: {what}: "));
            }
            // Every unreadable entry was reported before the load summed up.
            for diagnostic in diagnostics {
                assert!(
                    diagnostic.seq < summary.seq,
                    "a WARN was logged after `user keys bound`"
                );
            }

            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }

        // A file that is not valid JSON and one that is not an array: one WARN
        // naming the file each, and nothing bound.
        for (file, content) in [
            ("broken.json", "not json"),
            (
                "object.json",
                r#"{"bindings": {"ctrl-a": "keymap_test::Ping"}}"#,
            ),
        ] {
            let bad = temp.path().join(file);
            fs::write(&bad, content).unwrap();
            assert_eq!(cx.update(|cx| load_user_keymap(cx, &bad)), 0);
            let captured = CAPTURED.lock().unwrap();
            let groups = groups_since(&captured, groups_cursor, 1);
            let group = groups[0].id;
            assert_load_group(&groups[0], &captured.entered_groups, &bad);
            // The contract fixes only the WARN here: whether a file nothing
            // could be read from still ends in `user keys bound` is for the
            // implementation to say. If it logs one, it must be complete.
            let events: Vec<&Event> = captured.events[events_cursor..]
                .iter()
                .filter(|event| event.level != tracing::Level::TRACE)
                .collect();
            for event in &events {
                assert_eq!(event.group, Some(group));
                assert_eq!(event.level, tracing::Level::DEBUG);
                assert_eq!(event.message.as_deref(), Some("user keys bound"));
                assert_eq!(
                    event.fields.keys().copied().collect::<Vec<_>>().as_slice(),
                    ["count", "skipped"].as_slice(),
                    "unexpected fields in {event:#?}"
                );
                assert_eq!(event.fields["count"], "0");
            }
            assert!(events.len() <= 1, "unexpected events in {events:#?}");
            let diagnostics = diagnostics_since(&captured, diagnostics_cursor, 1);
            assert_warn(&diagnostics[0], group, "keymap.json:");
            groups_cursor = captured.groups.len();
            events_cursor = captured.events.len();
            diagnostics_cursor = captured.diagnostics.len();
        }
    });

    let captured = CAPTURED.lock().unwrap();
    assert!(
        !captured
            .entered
            .iter()
            .any(|id| captured.spans[id].starts_with("riso_keymap")),
        "an unclosed keymap-load group"
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
