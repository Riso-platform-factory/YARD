//! Trace groups for `yard_data`: `data-open{file}`, `data-migrate{extension, from, to}`,
//! and `data-request{extension, kind}` at `trace`.
//!
//! The database thread is a separate OS thread, so this binary installs a process-wide
//! subscriber and runs this single test alone.

use std::{cell::RefCell, collections::BTreeMap, sync::Mutex};
use yard_data::Database;

const A_STEP_1: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#;
const A_STEP_2: &str = r#"ALTER TABLE "a.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#;
const A_STEP_3: &str = r#"ALTER TABLE "a.t" ADD COLUMN note TEXT NOT NULL DEFAULT ''"#;
const A_TWO_STEPS: &[&str] = &[A_STEP_1, A_STEP_2];
const A_THREE_STEPS: &[&str] = &[A_STEP_1, A_STEP_2, A_STEP_3];

#[derive(Clone, Debug)]
struct SpanRecord {
    name: &'static str,
    level: tracing::Level,
    fields: String,
}

#[derive(Clone, Debug)]
struct EventRecord {
    span: Option<String>,
    level: tracing::Level,
}

#[derive(Default)]
struct Captured {
    spans_by_id: BTreeMap<u64, SpanRecord>,
    spans: Vec<SpanRecord>,
    events: Vec<EventRecord>,
}

static CAPTURED: Mutex<Captured> = Mutex::new(Captured {
    spans_by_id: BTreeMap::new(),
    spans: Vec::new(),
    events: Vec::new(),
});

thread_local! {
    static ENTERED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let rendered = format!("{value:?}").replace('"', "");
        write!(self.0, " {}={rendered}", field.name()).unwrap();
    }
}

struct Trace;

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let record = SpanRecord {
            name: attributes.metadata().name(),
            level: *attributes.metadata().level(),
            fields: fields.0,
        };
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        let id = captured.spans_by_id.len() as u64 + 1;
        if attributes.metadata().target().starts_with("yard_data") {
            captured.spans.push(record.clone());
        }
        captured.spans_by_id.insert(id, record);
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_data") {
            return;
        }
        let active = ENTERED.with(|stack| stack.borrow().last().copied());
        let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
        let span = active.and_then(|id| {
            captured
                .spans_by_id
                .get(&id)
                .map(|s| format!("{}{}", s.name, s.fields))
        });
        captured.events.push(EventRecord {
            span,
            level: *event.metadata().level(),
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        ENTERED.with(|stack| stack.borrow_mut().push(id.into_u64()));
    }

    fn exit(&self, _: &tracing::span::Id) {
        ENTERED.with(|stack| stack.borrow_mut().pop());
    }
}

fn take_captured() -> (Vec<SpanRecord>, Vec<EventRecord>) {
    let mut captured = CAPTURED.lock().unwrap_or_else(|e| e.into_inner());
    let spans = std::mem::take(&mut captured.spans);
    let events = std::mem::take(&mut captured.events);
    (spans, events)
}

#[test]
fn data_open_migrate_and_request_groups() {
    tracing::subscriber::set_global_default(Trace).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    // First open: opens the file once, runs 2 migrations (0 -> 2), then a write and a read.
    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(
                r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
                ("one", 1),
            )?;
            Ok(())
        })
        .wait()
        .unwrap();
        let _: i64 = db
            .read("a", |conn| {
                conn.query_row(r#"SELECT COUNT(*) FROM "a.t""#, [], |r| r.get(0))
            })
            .wait()
            .unwrap();
        db.flush();
    }

    let (spans, events) = take_captured();
    let opens: Vec<_> = spans.iter().filter(|s| s.name == "data-open").collect();
    assert_eq!(
        opens.len(),
        1,
        "expected `data-open` once per file open: {spans:#?}"
    );
    assert!(
        opens[0].fields.contains("file=") && opens[0].fields.contains("data.sqlite"),
        "`data-open` missing file field: {:?}",
        opens[0]
    );
    assert!(
        events.iter().any(|e| {
            e.span
                .as_deref()
                .is_some_and(|s| s.starts_with("data-open"))
                && e.level == tracing::Level::DEBUG
        }),
        "expected debug events inside `data-open`: {events:#?}"
    );

    let migrates: Vec<_> = spans.iter().filter(|s| s.name == "data-migrate").collect();
    assert_eq!(
        migrates.len(),
        1,
        "expected one `data-migrate` group when running 2 steps: {spans:#?}"
    );
    assert!(
        migrates[0].fields.contains("extension=a")
            && migrates[0].fields.contains("from=0")
            && migrates[0].fields.contains("to=2"),
        "`data-migrate` missing expected fields: {:?}",
        migrates[0]
    );
    assert!(
        events.iter().any(|e| {
            e.span
                .as_deref()
                .is_some_and(|s| s.starts_with("data-migrate"))
                && e.level == tracing::Level::DEBUG
        }),
        "expected debug events inside `data-migrate`: {events:#?}"
    );

    assert!(
        spans.iter().any(|s| {
            s.name == "data-request"
                && s.level == tracing::Level::TRACE
                && s.fields.contains("extension=a")
                && s.fields.contains("kind=write")
        }),
        "missing `data-request{{extension=a, kind=write}}` at trace: {spans:#?}"
    );
    assert!(
        spans.iter().any(|s| {
            s.name == "data-request"
                && s.level == tracing::Level::TRACE
                && s.fields.contains("extension=a")
                && s.fields.contains("kind=read")
        }),
        "missing `data-request{{extension=a, kind=read}}` at trace: {spans:#?}"
    );

    // Second open with the same steps: `data-open` once, NO `data-migrate`, `data-request` on read.
    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        let _: i64 = db
            .read("a", |conn| {
                conn.query_row(r#"SELECT COUNT(*) FROM "a.t""#, [], |r| r.get(0))
            })
            .wait()
            .unwrap();
        db.flush();
    }

    let (spans, _) = take_captured();
    assert_eq!(
        spans.iter().filter(|s| s.name == "data-open").count(),
        1,
        "expected `data-open` once on reopen: {spans:#?}"
    );
    assert_eq!(
        spans.iter().filter(|s| s.name == "data-migrate").count(),
        0,
        "expected no `data-migrate` group when there are no steps to run: {spans:#?}"
    );
    assert!(
        spans.iter().any(|s| {
            s.name == "data-request"
                && s.level == tracing::Level::TRACE
                && s.fields.contains("extension=a")
                && s.fields.contains("kind=read")
        }),
        "missing `data-request{{extension=a, kind=read}}` at trace on reopen: {spans:#?}"
    );

    // Third open with 1 new step: `data-migrate` with from=2, to=3.
    {
        let db = Database::new(file);
        db.migrate("a", A_THREE_STEPS).wait().unwrap();
        db.flush();
    }

    let (spans, _) = take_captured();
    let migrates: Vec<_> = spans.iter().filter(|s| s.name == "data-migrate").collect();
    assert_eq!(migrates.len(), 1, "expected one `data-migrate`: {spans:#?}");
    assert!(
        migrates[0].fields.contains("extension=a")
            && migrates[0].fields.contains("from=2")
            && migrates[0].fields.contains("to=3"),
        "`data-migrate` expected extension=a from=2 to=3: {:?}",
        migrates[0]
    );
}
