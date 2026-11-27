use gpui::TestAppContext;
use std::{thread, time::Duration};
use yard_shell::{self as shell, DataError, Db, rusqlite};

const NOTES_STEPS: &[&str] = &[
    r#"CREATE TABLE "notes.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#,
    r#"ALTER TABLE "notes.items" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#,
];
const ALPHA_STEPS: &[&str] =
    &[r#"CREATE TABLE "alpha.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];
const BETA_STEPS: &[&str] =
    &[r#"CREATE TABLE "beta.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];

// Through the handle: `set_data_file` with `Paths::data_file()`, `migrate_data` in init,
// then `write_data` and `read_data` run off the main thread after migrations and return
// what was written.
#[gpui::test]
async fn extension_migrates_writes_and_reads_off_main_thread(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let paths = yard_paths::Paths::under(dir.path());
    let file = paths.data_file();
    assert_eq!(file, dir.path().join("data/data.sqlite"));

    let main_thread = thread::current().id();
    let app = cx.new_app();
    let notes = app.update(|cx| {
        shell::set_data_file(cx, file.clone());
        let ext = shell::Extension::new(cx, "notes").unwrap();
        ext.migrate_data(cx, NOTES_STEPS);
        ext
    });

    // Queued immediately after `migrate_data`: must wait for migrations and run off main thread.
    let write_task = app.read(|cx| {
        notes.write_data(cx, move |db: &Db| {
            assert_ne!(
                thread::current().id(),
                main_thread,
                "write_data must run off the main thread"
            );
            db.execute(
                r#"INSERT INTO "notes.items" (text, done) VALUES (?1, ?2)"#,
                ("buy milk", 1),
            )?;
            Ok(())
        })
    });
    write_task.await.unwrap();

    let read_task = app.read(|cx| {
        notes.read_data(cx, move |db: &Db| {
            assert_ne!(
                thread::current().id(),
                main_thread,
                "read_data must run off the main thread"
            );
            let mut stmt = db.prepare(r#"SELECT text, done FROM "notes.items" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
    });
    let rows = read_task.await.unwrap();
    assert_eq!(rows, vec![("buy milk".to_owned(), 1)]);

    app.quit();
    app.run_until_parked();
}

// Quitting waits for writes already queued before quit, so they are in the file afterwards.
#[gpui::test]
async fn quitting_waits_for_queued_writes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let file = yard_paths::Paths::under(dir.path()).data_file();

    let app = cx.new_app();
    let notes = app.update(|cx| {
        shell::set_data_file(cx, file.clone());
        let ext = shell::Extension::new(cx, "notes").unwrap();
        ext.migrate_data(cx, NOTES_STEPS);
        ext
    });

    // Queue a brief slow write followed by several more writes right before quit without awaiting.
    app.read(|cx| {
        notes
            .write_data(cx, |db: &Db| {
                thread::sleep(Duration::from_millis(30));
                db.execute(
                    r#"INSERT INTO "notes.items" (text, done) VALUES (?1, ?2)"#,
                    ("item-00", 0),
                )?;
                Ok(())
            })
            .detach();
        for index in 1..=10usize {
            notes
                .write_data(cx, move |db: &Db| {
                    db.execute(
                        r#"INSERT INTO "notes.items" (text, done) VALUES (?1, ?2)"#,
                        (format!("item-{index:02}"), 1),
                    )?;
                    Ok(())
                })
                .detach();
        }
    });

    app.quit();
    app.run_until_parked();

    let raw = rusqlite::Connection::open(&file).unwrap();
    let rows = raw
        .prepare(r#"SELECT text FROM "notes.items" ORDER BY id"#)
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let expected: Vec<String> = (0..=10).map(|index| format!("item-{index:02}")).collect();
    assert_eq!(
        rows, expected,
        "every write queued before quit must be committed to the file"
    );
}

// Isolation through the handle: extension `alpha` cannot read extension `beta`'s table.
#[gpui::test]
async fn extension_cannot_read_another_extensions_table_through_handle(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let file = yard_paths::Paths::under(dir.path()).data_file();

    let app = cx.new_app();
    let (alpha, beta) = app.update(|cx| {
        shell::set_data_file(cx, file.clone());
        let alpha = shell::Extension::new(cx, "alpha").unwrap();
        let beta = shell::Extension::new(cx, "beta").unwrap();
        alpha.migrate_data(cx, ALPHA_STEPS);
        beta.migrate_data(cx, BETA_STEPS);
        (alpha, beta)
    });

    app.read(|cx| {
        beta.write_data(cx, |db: &Db| {
            db.execute(
                r#"INSERT INTO "beta.items" (text) VALUES (?1)"#,
                ["beta-only"],
            )?;
            Ok(())
        })
    })
    .await
    .unwrap();

    let peek = app
        .read(|cx| {
            alpha.read_data(cx, |db: &Db| {
                db.query_row(r#"SELECT text FROM "beta.items""#, [], |row| {
                    row.get::<_, String>(0)
                })
            })
        })
        .await;
    assert!(
        matches!(
            peek,
            Err(DataError::Sql(rusqlite::Error::SqliteFailure(ffi, _)))
                if ffi.code == rusqlite::ErrorCode::AuthorizationForStatementDenied
        ),
        "alpha reading beta's table must be refused by the authorizer, got {peek:?}"
    );

    let beta_text: String = app
        .read(|cx| {
            beta.read_data(cx, |db: &Db| {
                db.query_row(r#"SELECT text FROM "beta.items""#, [], |row| row.get(0))
            })
        })
        .await
        .unwrap();
    assert_eq!(beta_text, "beta-only");

    app.quit();
    app.run_until_parked();
}
