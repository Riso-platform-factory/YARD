#![cfg(feature = "dev")]

//! Diagnostic and trace levels for every failure row in `docs/src/development/data-store.md`.
//!
//! Installs a process-wide `log` logger and a process-wide `tracing` subscriber; alone in its binary.

use std::{fs, sync::Mutex};
use yard_data::{DataError, Database, rusqlite};

const A_STEP_1: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#;
const A_STEP_2: &str = r#"ALTER TABLE "a.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#;
const A_TWO_STEPS: &[&str] = &[A_STEP_1, A_STEP_2];
const A_ONE_STEP: &[&str] = &[A_STEP_1];
const A_EDITED_STEP_2: &[&str] = &[
    A_STEP_1,
    r#"ALTER TABLE "a.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 1"#,
];
const A_FAILING_STEP_2: &[&str] = &[A_STEP_1, "NOT VALID SQL AT ALL"];
const ALPHA_STEP_1: &str = r#"CREATE TABLE "alpha.t" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#;
const ALPHA_STEP_2: &str = r#"ALTER TABLE "alpha.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#;
const ALPHA_TWO_STEPS: &[&str] = &[ALPHA_STEP_1, ALPHA_STEP_2];
const ALPHA_ONE_STEP: &[&str] = &[ALPHA_STEP_1];
const B_ONE_STEP: &[&str] =
    &[r#"CREATE TABLE "b.t" (id INTEGER PRIMARY KEY, label TEXT NOT NULL)"#];

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());
static TRACE_EVENTS: Mutex<Vec<(tracing::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_data")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            DIAGNOSTICS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
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

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if !event.metadata().target().starts_with("yard_data") {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        TRACE_EVENTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn clear() {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

fn diagnostics() -> Vec<(log::Level, String)> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn trace_events() -> Vec<(tracing::Level, String)> {
    TRACE_EVENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn diag_errors() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .map(|(_, msg)| msg)
        .collect()
}

fn diag_warnings() -> Vec<String> {
    diagnostics()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Warn)
        .map(|(_, msg)| msg)
        .collect()
}

#[test]
fn failure_levels_match_contract_table() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let dir = tempfile::tempdir().unwrap();

    // 1. Random-bytes file: ERROR once in diagnostic log even across multiple requests.
    {
        clear();
        let corrupt = dir.path().join("corrupt.sqlite");
        fs::write(&corrupt, b"random-non-sqlite-bytes-0123456789abcdef").unwrap();
        let db = Database::new(corrupt);
        assert!(matches!(
            db.migrate("a", A_ONE_STEP).wait(),
            Err(DataError::Unavailable)
        ));
        assert!(matches!(
            db.read("a", |_| Ok(())).wait(),
            Err(DataError::Unavailable)
        ));
        db.flush();
        assert_eq!(
            diag_errors().len(),
            1,
            "corrupt file must log ERROR once: {:?}",
            diagnostics()
        );
        assert!(
            diag_warnings().is_empty(),
            "corrupt file must not log WARN: {:?}",
            diagnostics()
        );
    }

    // 2. Newer file: WARN once naming the extension and both step counts (1 and 2).
    {
        let newer = dir.path().join("newer.sqlite");
        {
            let db = Database::new(newer.clone());
            db.migrate("alpha", ALPHA_TWO_STEPS).wait().unwrap();
            db.flush();
        }
        clear();
        let db = Database::new(newer);
        assert!(matches!(
            db.migrate("alpha", ALPHA_ONE_STEP).wait(),
            Err(DataError::Newer {
                declared: 1,
                recorded: 2
            })
        ));
        assert!(matches!(
            db.read("alpha", |_| Ok(())).wait(),
            Err(DataError::Newer {
                declared: 1,
                recorded: 2
            })
        ));
        db.flush();
        let warns = diag_warnings();
        assert_eq!(
            warns.len(),
            1,
            "newer file must log WARN once: {:?}",
            diagnostics()
        );
        assert!(
            warns[0].contains("alpha") && warns[0].contains('1') && warns[0].contains('2'),
            "newer file WARN must name extension and both counts: {:?}",
            warns[0]
        );
        assert!(
            diag_errors().is_empty(),
            "newer file must not log ERROR: {:?}",
            diagnostics()
        );
    }

    // 3. Edited step: ERROR once.
    {
        let edited = dir.path().join("edited.sqlite");
        {
            let db = Database::new(edited.clone());
            db.migrate("a", A_TWO_STEPS).wait().unwrap();
            db.flush();
        }
        clear();
        let db = Database::new(edited);
        assert!(matches!(
            db.migrate("a", A_EDITED_STEP_2).wait(),
            Err(DataError::Migration)
        ));
        assert!(matches!(
            db.read("a", |_| Ok(())).wait(),
            Err(DataError::Migration)
        ));
        db.flush();
        assert_eq!(
            diag_errors().len(),
            1,
            "edited step must log ERROR once: {:?}",
            diagnostics()
        );
    }

    // 4. Failing step: ERROR once.
    {
        clear();
        let failing = dir.path().join("failing.sqlite");
        let db = Database::new(failing);
        assert!(matches!(
            db.migrate("a", A_FAILING_STEP_2).wait(),
            Err(DataError::Migration)
        ));
        assert!(matches!(
            db.read("a", |_| Ok(())).wait(),
            Err(DataError::Migration)
        ));
        db.flush();
        assert_eq!(
            diag_errors().len(),
            1,
            "failing step must log ERROR once: {:?}",
            diagnostics()
        );
    }

    // 5. Busy, 6. Authorizer refusal, 7. Another SQL error.
    {
        let good = dir.path().join("good.sqlite");
        let db = Database::new(good.clone());
        db.migrate("a", A_ONE_STEP).wait().unwrap();
        db.migrate("b", B_ONE_STEP).wait().unwrap();
        db.flush();

        // 5. Busy: WARN, no ERROR.
        clear();
        let locker = rusqlite::Connection::open(&good).unwrap();
        locker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert!(matches!(
            db.write("a", |tx| {
                tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["x"])?;
                Ok(())
            })
            .wait(),
            Err(DataError::Busy)
        ));
        locker.execute_batch("ROLLBACK").unwrap();
        drop(locker);
        db.flush();
        assert_eq!(
            diag_warnings().len(),
            1,
            "busy must log WARN: {:?}",
            diagnostics()
        );
        assert!(
            diag_errors().is_empty(),
            "busy must not log ERROR: {:?}",
            diagnostics()
        );

        // 6. Authorizer refusal: WARN, no ERROR.
        clear();
        let refused = db
            .read("a", |conn| {
                conn.query_row(r#"SELECT COUNT(*) FROM "b.t""#, [], |r| r.get::<_, i64>(0))
            })
            .wait();
        assert!(matches!(refused, Err(DataError::Sql(_))));
        db.flush();
        assert_eq!(
            diag_warnings().len(),
            1,
            "authorizer refusal must log WARN: {:?}",
            diagnostics()
        );
        assert!(
            diag_errors().is_empty(),
            "authorizer refusal must not log ERROR: {:?}",
            diagnostics()
        );

        // 7. Another SQL error in a closure: only `debug`, no diagnostic WARN or ERROR.
        clear();
        let other = db
            .read("a", |conn| {
                conn.query_row(r#"SELECT missing_col FROM "a.t""#, [], |r| {
                    r.get::<_, i64>(0)
                })
            })
            .wait();
        assert!(matches!(other, Err(DataError::Sql(_))));
        db.flush();
        assert!(
            diag_errors().is_empty(),
            "ordinary SQL error must not log diagnostic ERROR: {:?}",
            diagnostics()
        );
        assert!(
            diag_warnings().is_empty(),
            "ordinary SQL error must not log diagnostic WARN: {:?}",
            diagnostics()
        );
        let events = trace_events();
        assert!(
            events
                .iter()
                .any(|(level, _)| *level == tracing::Level::DEBUG),
            "ordinary SQL error must log at debug in trace: {events:#?}"
        );
        assert!(
            events
                .iter()
                .all(|(level, _)| *level != tracing::Level::ERROR && *level != tracing::Level::WARN),
            "ordinary SQL error must not log ERROR or WARN in trace: {events:#?}"
        );
    }
}
