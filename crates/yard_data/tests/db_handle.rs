#![cfg(feature = "dev")]

//! Schema rewriting is refused and closures receive `&yard_data::Db`, not `rusqlite`'s
//! `Connection` or `Transaction`.

use yard_data::{DataError, Database, Db, rusqlite};

const B_ITEMS_STEP: &[&str] =
    &[r#"CREATE TABLE "b.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];
const A_ITEMS_STEP: &[&str] =
    &[r#"CREATE TABLE "a.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#];

#[test]
fn schema_cannot_be_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");

    {
        let db = Database::new(file.clone());
        db.migrate("b", B_ITEMS_STEP).wait().unwrap();
        db.write("b", |db: &Db| {
            db.execute(r#"INSERT INTO "b.items" (text) VALUES (?1)"#, ["keep-b"])?;
            Ok(())
        })
        .wait()
        .unwrap();

        let update_master = db
            .write("a", |db: &Db| {
                db.execute(
                    r#"UPDATE sqlite_master SET sql = 'CREATE TABLE "b.items" (id INTEGER)' WHERE name = 'b.items'"#,
                    [],
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(update_master, Err(DataError::Sql(_))),
            "UPDATE sqlite_master must be refused, got {update_master:?}"
        );

        let delete_master = db
            .write("a", |db: &Db| {
                db.execute(r#"DELETE FROM sqlite_master WHERE name = 'b.items'"#, [])?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(delete_master, Err(DataError::Sql(_))),
            "DELETE FROM sqlite_master must be refused, got {delete_master:?}"
        );

        let writable_schema = db
            .write("a", |db: &Db| {
                db.execute_batch(
                    r#"PRAGMA writable_schema = ON; DELETE FROM sqlite_master WHERE name = 'b.items'"#,
                )?;
                Ok(())
            })
            .wait();
        assert!(
            matches!(writable_schema, Err(DataError::Sql(_))),
            "enabling writable_schema to delete from sqlite_master must be refused, got {writable_schema:?}"
        );

        db.flush();
    }

    // Reopen on the same file and verify `b` still reads its row intact.
    let db = Database::new(file);
    let text: String = db
        .read("b", |db: &Db| {
            db.query_row(r#"SELECT text FROM "b.items""#, [], |row| row.get(0))
        })
        .wait()
        .unwrap();
    assert_eq!(text, "keep-b");
}

#[test]
fn db_handle_exposes_contract_methods() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    let db = Database::new(file);
    db.migrate("a", A_ITEMS_STEP).wait().unwrap();

    let last_id = db
        .write("a", |db: &Db| {
            db.execute_batch(r#"INSERT INTO "a.items" (text) VALUES ('batch-row')"#)?;
            let inserted =
                db.execute(r#"INSERT INTO "a.items" (text) VALUES (?1)"#, ["exec-row"])?;
            assert_eq!(inserted, 1);
            Ok(db.last_insert_rowid())
        })
        .wait()
        .unwrap();
    assert_eq!(last_id, 2);

    let single: String = db
        .read("a", move |db: &Db| {
            db.query_row(
                r#"SELECT text FROM "a.items" WHERE id = ?1"#,
                [last_id],
                |row| row.get(0),
            )
        })
        .wait()
        .unwrap();
    assert_eq!(single, "exec-row");

    let all = db
        .read("a", |db: &Db| {
            let mut stmt = db.prepare(r#"SELECT text FROM "a.items" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| row.get::<_, String>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(all, vec!["batch-row".to_owned(), "exec-row".to_owned()]);
}

#[test]
fn last_insert_rowid_does_not_leak_across_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("data.sqlite"));
    db.migrate("a", A_ITEMS_STEP).wait().unwrap();
    db.migrate("b", B_ITEMS_STEP).wait().unwrap();

    let b_rowid = db
        .write("b", |db: &Db| {
            db.execute(
                r#"INSERT INTO "b.items" (id, text) VALUES (?1, ?2)"#,
                (42i64, "b-row"),
            )?;
            Ok(db.last_insert_rowid())
        })
        .wait()
        .unwrap();
    assert_eq!(b_rowid, 42);

    let a_read_rowid = db
        .read("a", |db: &Db| Ok(db.last_insert_rowid()))
        .wait()
        .unwrap();
    assert_eq!(
        a_read_rowid, 0,
        "read that inserted nothing must not see another extension's last_insert_rowid"
    );

    let a_write_rowid = db
        .write("a", |db: &Db| {
            db.execute(r#"INSERT INTO "a.items" (text) VALUES (?1)"#, ["a-row"])?;
            Ok(db.last_insert_rowid())
        })
        .wait()
        .unwrap();
    assert_eq!(a_write_rowid, 1);

    let stored_id: i64 = db
        .read("a", move |db: &Db| {
            db.query_row(
                r#"SELECT id FROM "a.items" WHERE id = ?1"#,
                [a_write_rowid],
                |row| row.get(0),
            )
        })
        .wait()
        .unwrap();
    assert_eq!(stored_id, 1);
}

fn is_auth_error(error: &DataError) -> bool {
    matches!(
        error,
        DataError::Sql(error)
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::AuthorizationForStatementDenied)
    )
}

#[test]
fn changes_and_total_changes_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("data.sqlite"));
    db.migrate("a", A_ITEMS_STEP).wait().unwrap();
    db.migrate("b", B_ITEMS_STEP).wait().unwrap();

    db.write("b", |db: &Db| {
        db.execute(
            r#"INSERT INTO "b.items" (text) VALUES (?1), (?2), (?3)"#,
            ["b-1", "b-2", "b-3"],
        )?;
        Ok(())
    })
    .wait()
    .unwrap();

    for sql in ["SELECT changes()", "SELECT total_changes()"] {
        let read = db
            .read("a", move |db: &Db| {
                db.query_row(sql, [], |row| row.get::<_, i64>(0))
            })
            .wait();
        assert!(
            read.as_ref().is_err_and(is_auth_error),
            "read `{sql}` must be refused with SQLite authorization error, got {read:?}"
        );
    }

    for sql in ["SELECT changes()", "SELECT total_changes()"] {
        let write = db
            .write("a", move |db: &Db| {
                db.query_row(sql, [], |row| row.get::<_, i64>(0))
            })
            .wait();
        assert!(
            write.as_ref().is_err_and(is_auth_error),
            "write `{sql}` must be refused with SQLite authorization error, got {write:?}"
        );
    }

    let b_rows = db
        .read("b", |db: &Db| {
            let mut stmt = db.prepare(r#"SELECT text FROM "b.items" ORDER BY id"#)?;
            let mapped = stmt.query_map([], |row| row.get::<_, String>(0))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()
        })
        .wait()
        .unwrap();
    assert_eq!(
        b_rows,
        vec!["b-1".to_owned(), "b-2".to_owned(), "b-3".to_owned()]
    );
}
