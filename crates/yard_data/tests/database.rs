use std::fs;
use yard_data::{DataError, Database, rusqlite};

const A_STEP_1: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#;
const A_STEP_2: &str = r#"ALTER TABLE "a.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#;
const A_STEP_3: &str = r#"ALTER TABLE "a.t" ADD COLUMN note TEXT NOT NULL DEFAULT ''"#;

const A_TWO_STEPS: &[&str] = &[A_STEP_1, A_STEP_2];
const A_THREE_STEPS: &[&str] = &[A_STEP_1, A_STEP_2, A_STEP_3];
const A_ONE_STEP: &[&str] = &[A_STEP_1];
const A_EDITED_STEP_2: &[&str] = &[
    A_STEP_1,
    r#"ALTER TABLE "a.t" ADD COLUMN done INTEGER NOT NULL DEFAULT 1"#,
];
const A_FAILING_STEP_2: &[&str] = &[A_STEP_1, "NOT VALID SQL AT ALL"];

const B_STEP_1: &str = r#"CREATE TABLE "b.t" (id INTEGER PRIMARY KEY, label TEXT NOT NULL)"#;
const B_ONE_STEP: &[&str] = &[B_STEP_1];

fn is_auth_error(error: &DataError) -> bool {
    matches!(
        error,
        DataError::Sql(rusqlite::Error::SqliteFailure(ffi, _))
            if ffi.code == rusqlite::ErrorCode::AuthorizationForStatementDenied
    )
}

#[test]
fn migrations_run_once_and_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(
                r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
                ("first", 1),
            )?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.flush();
    }

    // Reopening with the same steps runs nothing and preserves the row.
    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        let rows = db
            .read("a", |conn| {
                let mut stmt = conn.prepare(r#"SELECT text, done FROM "a.t" ORDER BY id"#)?;
                let mapped = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()
            })
            .wait()
            .unwrap();
        assert_eq!(rows, vec![("first".to_owned(), 1)]);
        db.flush();
    }

    // Reopening with one additional step runs only that step.
    {
        let db = Database::new(file.clone());
        db.migrate("a", A_THREE_STEPS).wait().unwrap();
        let rows = db
            .read("a", |conn| {
                let mut stmt = conn.prepare(r#"SELECT text, done, note FROM "a.t" ORDER BY id"#)?;
                let mapped = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()
            })
            .wait()
            .unwrap();
        assert_eq!(rows, vec![("first".to_owned(), 1, String::new())]);
        db.flush();
    }

    let raw = rusqlite::Connection::open(&file).unwrap();
    let mut stmt = raw
        .prepare(r#"SELECT sql FROM "yard.migrations" WHERE extension = 'a' ORDER BY step"#)
        .unwrap();
    let recorded = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(recorded, A_THREE_STEPS);
}

#[test]
fn two_extensions_steps_are_recorded_apart() {
    for a_first in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.sqlite");

        {
            let db = Database::new(file.clone());
            if a_first {
                db.migrate("a", A_TWO_STEPS).wait().unwrap();
                db.migrate("b", B_ONE_STEP).wait().unwrap();
            } else {
                db.migrate("b", B_ONE_STEP).wait().unwrap();
                db.migrate("a", A_TWO_STEPS).wait().unwrap();
            }
            db.write("a", |tx| {
                tx.execute(
                    r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
                    ("alpha", 1),
                )?;
                Ok(())
            })
            .wait()
            .unwrap();
            db.write("b", |tx| {
                tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["beta"])?;
                Ok(())
            })
            .wait()
            .unwrap();
            db.flush();
        }

        // Reopen and migrate in the opposite order; both extensions still work.
        let db = Database::new(file.clone());
        if a_first {
            db.migrate("b", B_ONE_STEP).wait().unwrap();
            db.migrate("a", A_TWO_STEPS).wait().unwrap();
        } else {
            db.migrate("a", A_TWO_STEPS).wait().unwrap();
            db.migrate("b", B_ONE_STEP).wait().unwrap();
        }
        let a_row: (String, i64) = db
            .read("a", |conn| {
                conn.query_row(r#"SELECT text, done FROM "a.t""#, [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
            })
            .wait()
            .unwrap();
        let b_row: String = db
            .read("b", |conn| {
                conn.query_row(r#"SELECT label FROM "b.t""#, [], |r| r.get(0))
            })
            .wait()
            .unwrap();
        assert_eq!(a_row, ("alpha".to_owned(), 1));
        assert_eq!(b_row, "beta");
    }
}

#[test]
fn newer_file_refuses_only_that_extension_and_keeps_rows() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        db.migrate("b", B_ONE_STEP).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(
                r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
                ("keep-me", 1),
            )?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.write("b", |tx| {
            tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["b-one"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.flush();
    }

    {
        let db = Database::new(file.clone());
        let mig = db.migrate("a", A_ONE_STEP).wait();
        assert!(
            matches!(
                mig,
                Err(DataError::Newer {
                    declared: 1,
                    recorded: 2
                })
            ),
            "expected Newer {{ declared: 1, recorded: 2 }}, got {mig:?}"
        );
        let read = db.read("a", |_| Ok(())).wait();
        assert!(
            matches!(
                read,
                Err(DataError::Newer {
                    declared: 1,
                    recorded: 2
                })
            ),
            "expected Newer on read, got {read:?}"
        );
        let write = db.write("a", |_| Ok(())).wait();
        assert!(
            matches!(
                write,
                Err(DataError::Newer {
                    declared: 1,
                    recorded: 2
                })
            ),
            "expected Newer on write, got {write:?}"
        );

        // Extension `b` still works normally.
        db.migrate("b", B_ONE_STEP).wait().unwrap();
        db.write("b", |tx| {
            tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["b-two"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        let b_rows = db
            .read("b", |conn| {
                let mut stmt = conn.prepare(r#"SELECT label FROM "b.t" ORDER BY id"#)?;
                let mapped = stmt.query_map([], |row| row.get::<_, String>(0))?;
                mapped.collect::<rusqlite::Result<Vec<_>>>()
            })
            .wait()
            .unwrap();
        assert_eq!(b_rows, vec!["b-one".to_owned(), "b-two".to_owned()]);
        db.flush();
    }

    // Verify `a`'s rows directly with plain `rusqlite` after dropping `Database`.
    let raw = rusqlite::Connection::open(&file).unwrap();
    let a_rows = raw
        .prepare(r#"SELECT text, done FROM "a.t" ORDER BY id"#)
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(a_rows, vec![("keep-me".to_owned(), 1)]);
}

#[test]
fn edited_step_refuses_only_that_extension() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", A_TWO_STEPS).wait().unwrap();
        db.migrate("b", B_ONE_STEP).wait().unwrap();
        db.write("b", |tx| {
            tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["still-good"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.flush();
    }

    let db = Database::new(file.clone());
    let mig = db.migrate("a", A_EDITED_STEP_2).wait();
    assert!(
        matches!(mig, Err(DataError::Migration)),
        "expected Migration for edited step, got {mig:?}"
    );
    let read = db.read("a", |_| Ok(())).wait();
    assert!(
        matches!(read, Err(DataError::Migration)),
        "expected Migration on read after edited step, got {read:?}"
    );
    let write = db.write("a", |_| Ok(())).wait();
    assert!(
        matches!(write, Err(DataError::Migration)),
        "expected Migration on write after edited step, got {write:?}"
    );

    // Extension `b` is unaffected.
    db.migrate("b", B_ONE_STEP).wait().unwrap();
    let b_label: String = db
        .read("b", |conn| {
            conn.query_row(r#"SELECT label FROM "b.t""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(b_label, "still-good");
}

#[test]
fn failing_step_rolls_back_and_is_not_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        let mig = db.migrate("a", A_FAILING_STEP_2).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "expected Migration for failing step, got {mig:?}"
        );
        let read = db.read("a", |_| Ok(())).wait();
        assert!(
            matches!(read, Err(DataError::Migration)),
            "expected Migration on read after failing step, got {read:?}"
        );
        db.flush();
    }

    // Step 1's table stayed, and only step 1 is recorded in `"yard.migrations"`.
    {
        let raw = rusqlite::Connection::open(&file).unwrap();
        let count: i64 = raw
            .query_row(
                r#"SELECT COUNT(*) FROM "yard.migrations" WHERE extension = 'a'"#,
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "failed step 2 must not be recorded");
        let table_exists: i64 = raw
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'a.t'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1, "step 1's table must stay");
    }

    // With a corrected step 2 on the next open, step 2 runs and succeeds.
    let db = Database::new(file.clone());
    db.migrate("a", A_TWO_STEPS).wait().unwrap();
    db.write("a", |tx| {
        tx.execute(
            r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
            ("recovered", 1),
        )?;
        Ok(())
    })
    .wait()
    .unwrap();
    let row: (String, i64) = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT text, done FROM "a.t""#, [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
        })
        .wait()
        .unwrap();
    assert_eq!(row, ("recovered".to_owned(), 1));
}

#[test]
fn write_transactions_roll_back_on_err_and_panic() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file);
    db.migrate("a", A_ONE_STEP).wait().unwrap();

    // Returning Err rolls back the transaction.
    let err_write = db
        .write("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["from-err"])?;
            Err::<(), _>(rusqlite::Error::InvalidQuery)
        })
        .wait();
    assert!(
        matches!(err_write, Err(DataError::Sql(_))),
        "expected Sql error, got {err_write:?}"
    );
    let count: i64 = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT COUNT(*) FROM "a.t""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(count, 0, "a write returning Err must roll back");

    // Panicking inside the closure returns Panicked, rolls back, and leaves the thread usable.
    let panic_write = db
        .write::<()>("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["from-panic"])?;
            panic!("boom inside write closure");
        })
        .wait();
    assert!(
        matches!(panic_write, Err(DataError::Panicked)),
        "expected Panicked, got {panic_write:?}"
    );
    let count: i64 = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT COUNT(*) FROM "a.t""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(count, 0, "a panicking write must roll back");

    // Subsequent requests still work.
    db.write("a", |tx| {
        tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["after-panic"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let text: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT text FROM "a.t""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(text, "after-panic");
}

#[test]
fn authorizer_enforces_extension_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file);

    db.migrate("a", A_ONE_STEP).wait().unwrap();
    db.migrate("b", B_ONE_STEP).wait().unwrap();
    db.write("b", |tx| {
        tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["b-secret"])?;
        Ok(())
    })
    .wait()
    .unwrap();

    // `a` reading `"b.t"` is refused by the authorizer.
    let read_b = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT label FROM "b.t""#, [], |r| r.get::<_, String>(0))
        })
        .wait();
    assert!(
        read_b.as_ref().is_err_and(is_auth_error),
        "expected authorizer Sql error reading b.t, got {read_b:?}"
    );

    // `a` writing `"b.t"` is refused and leaves `"b.t"` unchanged.
    let write_b = db
        .write("a", |tx| {
            tx.execute(r#"INSERT INTO "b.t" (label) VALUES (?1)"#, ["tampered"])?;
            Ok(())
        })
        .wait();
    assert!(
        write_b.as_ref().is_err_and(is_auth_error),
        "expected authorizer Sql error writing b.t, got {write_b:?}"
    );
    let b_rows = db
        .read("b", |conn| {
            let mut stmt = conn.prepare(r#"SELECT label FROM "b.t" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| row.get::<_, String>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(b_rows, vec!["b-secret".to_owned()]);

    // `a` creating an unprefixed table or another extension's table is refused.
    let create_plain = db
        .write("a", |tx| {
            tx.execute("CREATE TABLE plain (id INTEGER)", [])?;
            Ok(())
        })
        .wait();
    assert!(
        create_plain.as_ref().is_err_and(is_auth_error),
        "expected authorizer Sql error creating plain, got {create_plain:?}"
    );
    let create_b = db
        .write("a", |tx| {
            tx.execute(r#"CREATE TABLE "b.x" (id INTEGER)"#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        create_b.as_ref().is_err_and(is_auth_error),
        "expected authorizer Sql error creating b.x, got {create_b:?}"
    );

    // `a` accessing `"yard.migrations"` is refused.
    let read_migrations = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT COUNT(*) FROM "yard.migrations""#, [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .wait();
    assert!(
        read_migrations.as_ref().is_err_and(is_auth_error),
        "expected authorizer Sql error reading yard.migrations, got {read_migrations:?}"
    );

    // `a` on `"a.t"` works.
    db.write("a", |tx| {
        tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["a-ok"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let a_text: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT text FROM "a.t""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(a_text, "a-ok");
}

#[test]
fn unusable_file_returns_unavailable_and_is_untouched() {
    let dir = tempfile::tempdir().unwrap();

    // A file containing random non-SQLite bytes.
    let corrupt = dir.path().join("corrupt.sqlite");
    let original: Vec<u8> = (0u8..=255)
        .map(|b| b.wrapping_mul(37).wrapping_add(13))
        .collect();
    fs::write(&corrupt, &original).unwrap();
    {
        let db = Database::new(corrupt.clone());
        assert!(matches!(
            db.migrate("a", A_ONE_STEP).wait(),
            Err(DataError::Unavailable)
        ));
        assert!(matches!(
            db.read("a", |_| Ok(())).wait(),
            Err(DataError::Unavailable)
        ));
        assert!(matches!(
            db.write("a", |_| Ok(())).wait(),
            Err(DataError::Unavailable)
        ));
        db.flush();
    }
    assert_eq!(
        fs::read(&corrupt).unwrap(),
        original,
        "an unusable database file must remain byte for byte unchanged"
    );

    // A path under a regular file (parent directory does not exist and cannot be created).
    let blocker = dir.path().join("not-a-dir");
    fs::write(&blocker, b"file").unwrap();
    let unreachable = blocker.join("sub").join("data.sqlite");
    let db = Database::new(unreachable);
    assert!(matches!(
        db.migrate("a", A_ONE_STEP).wait(),
        Err(DataError::Unavailable)
    ));
    assert!(matches!(
        db.read("a", |_| Ok(())).wait(),
        Err(DataError::Unavailable)
    ));
    assert!(matches!(
        db.write("a", |_| Ok(())).wait(),
        Err(DataError::Unavailable)
    ));
}

#[test]
fn lock_contention_returns_busy_then_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file.clone());
    db.migrate("a", A_ONE_STEP).wait().unwrap();
    db.write("a", |tx| {
        tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["before-lock"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    db.flush();

    let locker = rusqlite::Connection::open(&file).unwrap();
    locker.execute_batch("BEGIN EXCLUSIVE").unwrap();

    let blocked = db
        .write("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["during-lock"])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(blocked, Err(DataError::Busy)),
        "expected Busy while another connection holds BEGIN EXCLUSIVE, got {blocked:?}"
    );

    locker.execute_batch("ROLLBACK").unwrap();
    drop(locker);

    db.write("a", |tx| {
        tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["after-lock"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let rows = db
        .read("a", |conn| {
            let mut stmt = conn.prepare(r#"SELECT text FROM "a.t" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| row.get::<_, String>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(
        rows,
        vec!["before-lock".to_owned(), "after-lock".to_owned()]
    );
}

#[test]
fn queued_writes_run_in_order_and_flush_waits() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file.clone());
    db.migrate("a", A_ONE_STEP).wait().unwrap();

    let count = 40usize;
    let replies: Vec<_> = (0..count)
        .map(|index| {
            db.write("a", move |tx| {
                tx.execute(
                    r#"INSERT INTO "a.t" (text) VALUES (?1)"#,
                    [format!("item-{index:02}")],
                )?;
                Ok(())
            })
        })
        .collect();

    db.flush();

    // Every queued write must already be committed in order once `flush()` returns.
    let raw = rusqlite::Connection::open(&file).unwrap();
    let rows = raw
        .prepare(r#"SELECT text FROM "a.t" ORDER BY id"#)
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let expected: Vec<String> = (0..count).map(|index| format!("item-{index:02}")).collect();
    assert_eq!(rows, expected);

    for reply in replies {
        reply.wait().unwrap();
    }
}

#[test]
fn no_foreign_key_into_other_extension_or_migrations() {
    const B_ITEMS: &[&str] =
        &[r#"CREATE TABLE "b.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];
    const A_FK_CREATE_MIGRATION: &[&str] =
        &[r#"CREATE TABLE "a.fk" (r INTEGER REFERENCES "b.items"(id))"#];
    const A_FK_ALTER_MIGRATION: &[&str] = &[
        A_STEP_1,
        r#"ALTER TABLE "a.t" ADD COLUMN r INTEGER REFERENCES "b.items"(id)"#,
    ];
    const A_FK_YARD_MIGRATIONS: &[&str] = &[
        r#"CREATE TABLE "a.fk_mig" (ext TEXT, step INTEGER, FOREIGN KEY (ext, step) REFERENCES "yard.migrations"(extension, step))"#,
    ];

    // 1. Cross-extension FK in CREATE TABLE migration step is refused with DataError::Migration.
    {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("data.sqlite"));
        db.migrate("b", B_ITEMS).wait().unwrap();
        db.write("b", |tx| {
            tx.execute(
                r#"INSERT INTO "b.items" (id, text) VALUES (1, 'owned')"#,
                [],
            )?;
            Ok(())
        })
        .wait()
        .unwrap();

        let mig = db.migrate("a", A_FK_CREATE_MIGRATION).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "FK to another extension's table in migration must fail with DataError::Migration, got {mig:?}"
        );

        let deleted = db
            .write("b", |tx| {
                tx.execute(r#"DELETE FROM "b.items" WHERE id = 1"#, [])
            })
            .wait()
            .unwrap();
        assert_eq!(deleted, 1, "b must still be able to delete its own row");
    }

    // 2. Cross-extension FK in ALTER TABLE migration step is refused with DataError::Migration.
    {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("data.sqlite"));
        db.migrate("b", B_ITEMS).wait().unwrap();
        db.write("b", |tx| {
            tx.execute(
                r#"INSERT INTO "b.items" (id, text) VALUES (1, 'owned')"#,
                [],
            )?;
            Ok(())
        })
        .wait()
        .unwrap();

        let mig = db.migrate("a", A_FK_ALTER_MIGRATION).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "ALTER TABLE adding FK to another extension's table in migration must fail, got {mig:?}"
        );

        let deleted = db
            .write("b", |tx| {
                tx.execute(r#"DELETE FROM "b.items" WHERE id = 1"#, [])
            })
            .wait()
            .unwrap();
        assert_eq!(deleted, 1, "b must still be able to delete its own row");
    }

    // 3. FK to `"yard.migrations"` in a migration step is refused with DataError::Migration.
    {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("data.sqlite"));
        let mig = db.migrate("a", A_FK_YARD_MIGRATIONS).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "FK to yard.migrations in migration must fail with DataError::Migration, got {mig:?}"
        );
    }

    // 4. Inside writes: cross-extension FKs and FKs to `"yard.migrations"` are refused,
    //    `b` can delete its row, and an FK between two of `a`'s own tables works.
    {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("data.sqlite"));
        db.migrate("b", B_ITEMS).wait().unwrap();
        db.migrate("a", A_ONE_STEP).wait().unwrap();
        db.write("b", |tx| {
            tx.execute(
                r#"INSERT INTO "b.items" (id, text) VALUES (1, 'owned')"#,
                [],
            )?;
            Ok(())
        })
        .wait()
        .unwrap();

        let create_fk = db
            .write("a", |tx| {
                tx.execute(
                    r#"CREATE TABLE "a.fk" (r INTEGER REFERENCES "b.items"(id))"#,
                    [],
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(create_fk, Err(DataError::Sql(_))),
            "CREATE TABLE with FK to b.items inside write must be refused, got {create_fk:?}"
        );

        let alter_fk = db
            .write("a", |tx| {
                tx.execute(
                    r#"ALTER TABLE "a.t" ADD COLUMN r REFERENCES "b.items"(id)"#,
                    [],
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(alter_fk, Err(DataError::Sql(_))),
            "ALTER TABLE ADD COLUMN with FK to b.items inside write must be refused, got {alter_fk:?}"
        );

        let mig_fk = db
            .write("a", |tx| {
                tx.execute(
                    r#"CREATE TABLE "a.fk_mig" (ext TEXT, step INTEGER, FOREIGN KEY (ext, step) REFERENCES "yard.migrations"(extension, step))"#,
                    [],
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(mig_fk, Err(DataError::Sql(_))),
            "FK to yard.migrations inside write must be refused, got {mig_fk:?}"
        );

        // `b` can delete its own row.
        let deleted = db
            .write("b", |tx| {
                tx.execute(r#"DELETE FROM "b.items" WHERE id = 1"#, [])
            })
            .wait()
            .unwrap();
        assert_eq!(deleted, 1, "b must be able to delete its own row");

        // A foreign key between two of `a`'s own tables still works and is enforced.
        db.write("a", |tx| {
            tx.execute(r#"CREATE TABLE "a.parent" (id INTEGER PRIMARY KEY)"#, [])?;
            tx.execute(
                r#"CREATE TABLE "a.child" (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES "a.parent"(id))"#,
                [],
            )?;
            tx.execute(r#"INSERT INTO "a.parent" (id) VALUES (1)"#, [])?;
            tx.execute(
                r#"INSERT INTO "a.child" (id, parent_id) VALUES (10, 1)"#,
                [],
            )?;
            Ok(())
        })
        .wait()
        .unwrap();

        let parent_id: i64 = db
            .read("a", |conn| {
                conn.query_row(
                    r#"SELECT parent_id FROM "a.child" WHERE id = 10"#,
                    [],
                    |r| r.get(0),
                )
            })
            .wait()
            .unwrap();
        assert_eq!(parent_id, 1);

        let dangling = db
            .write("a", |tx| {
                tx.execute(
                    r#"INSERT INTO "a.child" (id, parent_id) VALUES (11, 999)"#,
                    [],
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(dangling, Err(DataError::Sql(_))),
            "own-table FK constraint must still be enforced, got {dangling:?}"
        );
    }
}

#[test]
fn migration_meeting_busy_runs_again_on_next_request() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file.clone());
    db.migrate("b", B_ONE_STEP).wait().unwrap();
    db.flush();

    let locker = rusqlite::Connection::open(&file).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();

    let busy = db.migrate("a", A_TWO_STEPS).wait();
    assert!(
        matches!(busy, Err(DataError::Busy)),
        "expected Busy while another connection holds BEGIN IMMEDIATE, got {busy:?}"
    );

    locker.execute_batch("ROLLBACK").unwrap();
    drop(locker);

    // Next request for `a` must see the migrated schema (including step 2's `done` column).
    let initial_rows = db
        .read("a", |conn| {
            let mut stmt = conn.prepare(r#"SELECT text, done FROM "a.t" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert!(initial_rows.is_empty());

    db.write("a", |tx| {
        tx.execute(
            r#"INSERT INTO "a.t" (text, done) VALUES (?1, ?2)"#,
            ("after-busy", 1),
        )?;
        Ok(())
    })
    .wait()
    .unwrap();
    db.flush();

    let raw = rusqlite::Connection::open(&file).unwrap();
    let recorded = raw
        .prepare(r#"SELECT step, sql FROM "yard.migrations" WHERE extension = 'a' ORDER BY step"#)
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        recorded,
        vec![(0, A_STEP_1.to_owned()), (1, A_STEP_2.to_owned())],
        "ledger must record both steps once each"
    );
}

#[test]
fn own_table_sql_allowed_and_unsafe_features_refused() {
    const B_ITEMS: &[&str] =
        &[r#"CREATE TABLE "b.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];
    const A_INIT: &[&str] = &[
        A_STEP_1,
        r#"CREATE TABLE "a.x" (id INTEGER PRIMARY KEY, val TEXT NOT NULL)"#,
    ];

    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("data.sqlite"));
    db.migrate("b", B_ITEMS).wait().unwrap();
    db.migrate("a", A_INIT).wait().unwrap();
    db.write("a", |tx| {
        tx.execute(r#"INSERT INTO "a.t" (text) VALUES (?1)"#, ["row-t"])?;
        tx.execute(r#"INSERT INTO "a.x" (val) VALUES (?1)"#, ["before-rename"])?;
        Ok(())
    })
    .wait()
    .unwrap();

    // Renaming to another extension's name or an unowned name is refused.
    let rename_to_b = db
        .write("a", |tx| {
            tx.execute(r#"ALTER TABLE "a.x" RENAME TO "b.y""#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(rename_to_b, Err(DataError::Sql(_))),
        "RENAME TO another extension's table must be refused, got {rename_to_b:?}"
    );

    let rename_to_plain = db
        .write("a", |tx| {
            tx.execute(r#"ALTER TABLE "a.x" RENAME TO "plain""#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(rename_to_plain, Err(DataError::Sql(_))),
        "RENAME TO an unprefixed name must be refused, got {rename_to_plain:?}"
    );

    // Renaming to a name `a` owns works, and `"a.y"` is readable afterwards.
    db.write("a", |tx| {
        tx.execute(r#"ALTER TABLE "a.x" RENAME TO "a.y""#, [])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let renamed_val: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT val FROM "a.y""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(renamed_val, "before-rename");

    // AUTOINCREMENT table creation and inserts work.
    db.write("a", |tx| {
        tx.execute(
            r#"CREATE TABLE "a.ai" (id INTEGER PRIMARY KEY AUTOINCREMENT)"#,
            [],
        )?;
        tx.execute(r#"INSERT INTO "a.ai" DEFAULT VALUES"#, [])?;
        tx.execute(r#"INSERT INTO "a.ai" DEFAULT VALUES"#, [])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let ai_ids = db
        .read("a", |conn| {
            let mut stmt = conn.prepare(r#"SELECT id FROM "a.ai" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(ai_ids, vec![1, 2]);

    // ANALYZE of `a`'s own table works.
    db.write("a", |tx| {
        tx.execute(r#"ANALYZE "a.t""#, [])?;
        Ok(())
    })
    .wait()
    .unwrap();

    // `json_each` and `json_tree` table-valued functions work.
    let json_values = db
        .read("a", |conn| {
            let mut stmt = conn.prepare("SELECT value FROM json_each('[1,2]')")?;
            let mapped = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(json_values, vec![1, 2]);

    let tree_value: i64 = db
        .read("a", |conn| {
            conn.query_row(
                r#"SELECT value FROM json_tree('{"k":1}') WHERE key = 'k'"#,
                [],
                |row| row.get(0),
            )
        })
        .wait()
        .unwrap();
    assert_eq!(tree_value, 1);

    // Virtual tables (FTS5), `dbstat`, and `pragma_*` functions remain refused.
    let fts5 = db
        .write("a", |tx| {
            tx.execute(r#"CREATE VIRTUAL TABLE "a.f" USING fts5(x)"#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(fts5, Err(DataError::Sql(_))),
        "FTS5 virtual table must be refused, got {fts5:?}"
    );

    let dbstat = db
        .read("a", |conn| {
            let mut stmt = conn.prepare("SELECT * FROM dbstat")?;
            stmt.query_map([], |_| Ok(()))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait();
    assert!(
        matches!(dbstat, Err(DataError::Sql(_))),
        "SELECT FROM dbstat must be refused, got {dbstat:?}"
    );

    let pragma_fn = db
        .read("a", |conn| {
            let mut stmt = conn.prepare("SELECT * FROM pragma_table_info('b.items')")?;
            stmt.query_map([], |_| Ok(()))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait();
    assert!(
        matches!(pragma_fn, Err(DataError::Sql(_))),
        "SELECT FROM pragma_table_info must be refused, got {pragma_fn:?}"
    );
}

#[test]
fn view_over_other_extension_table_is_refused() {
    const A_BASE: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, x TEXT NOT NULL)"#;
    const A_RENAMES: &[&str] = &[
        A_BASE,
        r#"ALTER TABLE "a.t" RENAME TO "a.notes""#,
        r#"ALTER TABLE "a.notes" RENAME COLUMN x TO body"#,
    ];
    const B_BASE: &str = r#"CREATE TABLE "b.t" (id INTEGER PRIMARY KEY, y TEXT NOT NULL)"#;
    const B_BAD_VIEW: &[&str] = &[B_BASE, r#"CREATE VIEW "b.v" AS SELECT x FROM "a.t""#];
    const B_OWN_VIEW: &[&str] = &[B_BASE, r#"CREATE VIEW "b.own_v" AS SELECT y FROM "b.t""#];

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", &[A_BASE]).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (x) VALUES (?1)"#, ["seed"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.migrate("b", &[B_BASE]).wait().unwrap();
        db.flush();
    }

    // Refused as a migration step.
    {
        let db = Database::new(file.clone());
        let mig = db.migrate("b", B_BAD_VIEW).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "CREATE VIEW over another extension's table in migration must fail with DataError::Migration, got {mig:?}"
        );
        db.flush();
    }

    // Refused inside a write, then `a` can rename its table and column, and `b`'s own view works.
    let db = Database::new(file);
    let write_view = db
        .write("b", |tx| {
            tx.execute(r#"CREATE VIEW "b.v" AS SELECT x FROM "a.t""#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(write_view, Err(DataError::Sql(_))),
        "CREATE VIEW over another extension's table inside write must fail with DataError::Sql, got {write_view:?}"
    );

    db.migrate("a", A_RENAMES).wait().unwrap();
    let body: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT body FROM "a.notes""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(body, "seed");

    db.migrate("b", B_OWN_VIEW).wait().unwrap();
    db.write("b", |tx| {
        tx.execute(r#"INSERT INTO "b.t" (y) VALUES (?1)"#, ["b-row"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let own_val: String = db
        .read("b", |conn| {
            conn.query_row(r#"SELECT y FROM "b.own_v""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(own_val, "b-row");
}

#[test]
fn temp_view_over_other_extension_table_is_refused() {
    const A_BASE: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, x TEXT NOT NULL)"#;
    const A_RENAMES: &[&str] = &[
        A_BASE,
        r#"ALTER TABLE "a.t" RENAME TO "a.notes""#,
        r#"ALTER TABLE "a.notes" RENAME COLUMN x TO body"#,
    ];
    const B_BASE: &str = r#"CREATE TABLE "b.t" (id INTEGER PRIMARY KEY, y TEXT NOT NULL)"#;
    const B_BAD_TEMP_VIEW: &[&str] = &[B_BASE, r#"CREATE TEMP VIEW "b.tv" AS SELECT x FROM "a.t""#];

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", &[A_BASE]).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (x) VALUES (?1)"#, ["seed"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.migrate("b", &[B_BASE]).wait().unwrap();
        db.flush();
    }

    // Refused as a migration step.
    {
        let db = Database::new(file.clone());
        let mig = db.migrate("b", B_BAD_TEMP_VIEW).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "CREATE TEMP VIEW over another extension's table in migration must fail with DataError::Migration, got {mig:?}"
        );
        db.flush();
    }

    // Refused inside a write, then `a` can rename its table and column on the same connection,
    // and `b`'s own temp view works.
    let db = Database::new(file);
    let write_temp_view = db
        .write("b", |tx| {
            tx.execute(r#"CREATE TEMP VIEW "b.tv" AS SELECT x FROM "a.t""#, [])?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(write_temp_view, Err(DataError::Sql(_))),
        "CREATE TEMP VIEW over another extension's table inside write must fail with DataError::Sql, got {write_temp_view:?}"
    );

    db.migrate("a", A_RENAMES).wait().unwrap();
    let body: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT body FROM "a.notes""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(body, "seed");

    db.write("b", |tx| {
        tx.execute(r#"INSERT INTO "b.t" (y) VALUES (?1)"#, ["b-temp"])?;
        tx.execute(r#"CREATE TEMP VIEW "b.own_tv" AS SELECT y FROM "b.t""#, [])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let own_val: String = db
        .read("b", |conn| {
            conn.query_row(r#"SELECT y FROM "b.own_tv""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(own_val, "b-temp");
}

#[test]
fn trigger_naming_other_extension_table_is_refused() {
    const A_BASE: &str = r#"CREATE TABLE "a.t" (id INTEGER PRIMARY KEY, x TEXT NOT NULL)"#;
    const A_RENAMES: &[&str] = &[
        A_BASE,
        r#"ALTER TABLE "a.t" RENAME TO "a.notes""#,
        r#"ALTER TABLE "a.notes" RENAME COLUMN x TO body"#,
    ];
    const B_BASE: &str = r#"CREATE TABLE "b.t" (id INTEGER PRIMARY KEY, y TEXT NOT NULL)"#;
    const B_BAD_TRIGGER_READ: &[&str] = &[
        B_BASE,
        r#"CREATE TRIGGER "b.tr_read" AFTER INSERT ON "b.t" BEGIN SELECT x FROM "a.t"; END"#,
    ];
    const B_BAD_TRIGGER_WRITE: &[&str] = &[
        B_BASE,
        r#"CREATE TRIGGER "b.tr_write" AFTER INSERT ON "b.t" BEGIN INSERT INTO "a.t" (x) VALUES (NEW.y); END"#,
    ];
    const B_OWN_TRIGGER: &[&str] = &[
        B_BASE,
        r#"CREATE TABLE "b.log" (id INTEGER PRIMARY KEY, msg TEXT NOT NULL)"#,
        r#"CREATE TRIGGER "b.own_tr" AFTER INSERT ON "b.t" BEGIN INSERT INTO "b.log" (msg) SELECT y FROM "b.t" WHERE id = NEW.id; END"#,
    ];

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("a", &[A_BASE]).wait().unwrap();
        db.write("a", |tx| {
            tx.execute(r#"INSERT INTO "a.t" (x) VALUES (?1)"#, ["seed"])?;
            Ok(())
        })
        .wait()
        .unwrap();
        db.migrate("b", &[B_BASE]).wait().unwrap();
        db.flush();
    }

    // Trigger whose body reads `"a.t"` is refused as a migration step.
    {
        let db = Database::new(file.clone());
        let mig = db.migrate("b", B_BAD_TRIGGER_READ).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "CREATE TRIGGER reading another extension's table in migration must fail with DataError::Migration, got {mig:?}"
        );
        db.flush();
    }

    // Trigger whose body writes `"a.t"` is refused as a migration step.
    {
        let db = Database::new(file.clone());
        let mig = db.migrate("b", B_BAD_TRIGGER_WRITE).wait();
        assert!(
            matches!(mig, Err(DataError::Migration)),
            "CREATE TRIGGER writing another extension's table in migration must fail with DataError::Migration, got {mig:?}"
        );
        db.flush();
    }

    // Both trigger forms are refused inside writes, then `a` can rename its table and column,
    // and `b`'s own-table trigger still works.
    let db = Database::new(file);
    let write_read_trigger = db
        .write("b", |tx| {
            tx.execute(
                r#"CREATE TRIGGER "b.tr_read" AFTER INSERT ON "b.t" BEGIN SELECT x FROM "a.t"; END"#,
                [],
            )?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(write_read_trigger, Err(DataError::Sql(_))),
        "CREATE TRIGGER reading another extension's table inside write must fail with DataError::Sql, got {write_read_trigger:?}"
    );

    let write_write_trigger = db
        .write("b", |tx| {
            tx.execute(
                r#"CREATE TRIGGER "b.tr_write" AFTER INSERT ON "b.t" BEGIN INSERT INTO "a.t" (x) VALUES (NEW.y); END"#,
                [],
            )?;
            Ok(())
        })
        .wait();
    assert!(
        matches!(write_write_trigger, Err(DataError::Sql(_))),
        "CREATE TRIGGER writing another extension's table inside write must fail with DataError::Sql, got {write_write_trigger:?}"
    );

    db.migrate("a", A_RENAMES).wait().unwrap();
    let body: String = db
        .read("a", |conn| {
            conn.query_row(r#"SELECT body FROM "a.notes""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(body, "seed");

    db.migrate("b", B_OWN_TRIGGER).wait().unwrap();
    db.write("b", |tx| {
        tx.execute(r#"INSERT INTO "b.t" (y) VALUES (?1)"#, ["logged"])?;
        Ok(())
    })
    .wait()
    .unwrap();
    let logged: String = db
        .read("b", |conn| {
            conn.query_row(r#"SELECT msg FROM "b.log""#, [], |r| r.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(logged, "logged");
}
