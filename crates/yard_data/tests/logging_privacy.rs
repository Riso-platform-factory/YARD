#![cfg(feature = "dev")]

//! Neither the lifecycle trace nor the diagnostic log may record SQL text or bound values.
//!
//! Installs a process-wide `log` logger and a process-wide `tracing` subscriber; alone in its binary.

use std::sync::Mutex;
use yard_data::{DataError, Database};

const SECRET_TABLE_TOKEN: &str = "secret_notes_9c4e";
const SECRET_VALUE: &str = "secret-value-7f3a";
const SECRET_REJECTED: &str = "secret-rejected-8b2c";
const SECRET_DENIED: &str = "secret-denied-5e1d";

const ALPHA_STEPS: &[&str] = &[
    r#"CREATE TABLE "alpha.secret_notes_9c4e" (id INTEGER PRIMARY KEY, body TEXT NOT NULL CHECK (length(body) > 5))"#,
];
const BETA_STEPS: &[&str] = &[r#"CREATE TABLE "beta.other" (id INTEGER PRIMARY KEY, body TEXT)"#];

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
            "log {}: {}",
            record.level(),
            record.args()
        ));
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

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
            "span {} {}{}",
            attributes.metadata().level(),
            attributes.metadata().name(),
            fields.0
        ));
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        LOGS.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
            "event {}{}",
            event.metadata().level(),
            fields.0
        ));
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn neither_log_contains_sql_text_or_bound_values() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(Trace).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file);

    db.migrate("alpha", ALPHA_STEPS).wait().unwrap();
    db.migrate("beta", BETA_STEPS).wait().unwrap();

    db.write("alpha", |tx| {
        tx.execute(
            r#"INSERT INTO "alpha.secret_notes_9c4e" (body) VALUES (?1)"#,
            [SECRET_VALUE],
        )?;
        Ok(())
    })
    .wait()
    .unwrap();

    let found: String = db
        .read("alpha", |conn| {
            conn.query_row(
                r#"SELECT body FROM "alpha.secret_notes_9c4e" WHERE body = ?1"#,
                [SECRET_VALUE],
                |row| row.get(0),
            )
        })
        .wait()
        .unwrap();
    assert_eq!(found, SECRET_VALUE);

    // A failing write with a bound value (CHECK constraint violation).
    let failed = db
        .write("alpha", |tx| {
            tx.execute(
                r#"INSERT INTO "alpha.secret_notes_9c4e" (body) VALUES (substr(?1, 1, 2))"#,
                [SECRET_REJECTED],
            )?;
            Ok(())
        })
        .wait();
    assert!(matches!(failed, Err(DataError::Sql(_))));

    // An authorizer refusal with a bound value.
    let denied = db
        .write("alpha", |tx| {
            tx.execute(
                r#"INSERT INTO "beta.other" (body) VALUES (?1)"#,
                [SECRET_DENIED],
            )?;
            Ok(())
        })
        .wait();
    assert!(matches!(denied, Err(DataError::Sql(_))));

    db.flush();

    let entries = LOGS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for needle in [
        SECRET_TABLE_TOKEN,
        SECRET_VALUE,
        SECRET_REJECTED,
        SECRET_DENIED,
        "CREATE TABLE",
        "INSERT INTO",
        "SELECT body",
    ] {
        assert!(
            entries.iter().all(|entry| !entry.contains(needle)),
            "log leaked sensitive SQL or bound value ({needle}): {entries:#?}"
        );
    }
}
