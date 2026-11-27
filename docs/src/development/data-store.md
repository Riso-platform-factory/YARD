# data-store: a queryable store for an application's own data

An application built on YARD keeps records, lists, and history: notes, tasks, a log of what the user did. Today an extension has the node state (`state.json`), its key-value store (`store/<extension>.json`), and the settings, all JSON files read whole into memory, with no query, no transaction, and no way to change their shape over time. This slice gives each extension an SQLite database through the extension handle: its own tables in one file per application, migrations it declares, every request off the UI thread, and the user's data never thrown away when something goes wrong.

The design follows Zed's `db` and `sqlez` (one SQLite file shared by every part of the application, migrations declared per domain and recorded in the database, a writer thread, a key-value table beside the relational ones), read in the research for this slice. Those crates are GPL, so YARD writes its own on [`rusqlite`](https://crates.io/crates/rusqlite) (MIT) with SQLite bundled, so no system library is needed on macOS, Linux, or Windows.

## What an extension writes

```rust
const MIGRATIONS: &[&str] = &[
    r#"CREATE TABLE "notes.items" (id INTEGER PRIMARY KEY, text TEXT NOT NULL)"#,
    r#"ALTER TABLE "notes.items" ADD COLUMN done INTEGER NOT NULL DEFAULT 0"#,
];

pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    ext.migrate_data(cx, MIGRATIONS);
}

// Later, from a view or an action:
let task = ext.write_data(cx, move |db| {
    db.execute(r#"INSERT INTO "notes.items" (text) VALUES (?1)"#, [text])?;
    Ok(())
});
let items = ext.read_data(cx, |db| {
    let mut query = db.prepare(r#"SELECT id, text, done FROM "notes.items" ORDER BY id"#)?;
    let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect::<Result<Vec<(i64, String, bool)>, _>>()
});
```

- **One file per application**, `<data>/data.sqlite`, beside `state.json` and `store/`. The startup gives its path to the shell, as it gives the store folder (`set_data_file`). Release channels (#6) will give each channel its own data folder, so the name has no channel in it.
- **SQL, on the extension's own tables only.** The closure gets YARD's handle `Db`, not rusqlite's connection. `Db` has `execute`, `execute_batch`, `prepare`, `query_row`, and `last_insert_rowid`, with rusqlite's signatures. It has no way to the connection itself, so a closure cannot change a setting that every extension shares. `yard_shell` re-exports `Db`, `DataError`, and `rusqlite` (for parameters, rows, and errors), so an extension adds no dependency. Every table, index, view, and trigger an extension creates or touches is named `"<extension>.<name>"`, the shape of a component ID. An extension's name never has a `.` (YARD's slugs allow only letters, digits, `-`, and `_`), so the owner of a table is always the part before the first `.`, and SQL needs the quotes. The shell checks every statement with SQLite's authorizer: a statement on another extension's table, or on one without its owner's prefix, is refused before it runs, and the closure gets an error. The shell's own table, `"yard.migrations"`, can only be read by the shell.
  - **Allowed on the extension's own tables:** what SQLite allows, including the following.
    - `ALTER TABLE ... RENAME TO` another name the extension owns, and `RENAME COLUMN`.
    - `AUTOINCREMENT`, and `ANALYZE` of its own table.
    - Views and triggers over its own tables.
    - The table-valued functions `json_each` and `json_tree`.
  - **Refused**, each as a statement on a table the extension does not own:
    - a view, trigger, or foreign key (`REFERENCES`) that names another extension's table or `"yard.migrations"`;
    - direct writes to `sqlite_master`;
    - virtual tables, FTS5 among them;
    - `dbstat` and the `pragma_*` functions;
    - `ATTACH`, `PRAGMA`, `VACUUM`, and transaction statements;
    - `load_extension`;
    - `changes()` and `total_changes()`, which would show another extension's activity on the shared connection.
  - `last_insert_rowid` starts at `0` in every request.
- **Migrations**, in `init`: an ordered list of SQL steps, each run once, in a transaction, recorded in `"yard.migrations"` (`extension`, `step`, `sql`). They only go forward: a step is never undone and never edited after it shipped; a new change is a new step. Requests of that extension wait until its migrations are done. A migration stopped by `Busy` runs again before that extension's next request. An extension that never calls `migrate_data` can still read and write tables made by its earlier steps; one without tables gets "no such table" from SQLite.
- **Never on the UI thread.** `read_data` and `write_data` return a `gpui::Task<Result<T, DataError>>`. One thread owns the connection and runs every request in the order it came. Each request runs in a transaction; a `write_data` closure's transaction that is committed when it returns `Ok` and rolled back when it returns `Err` or panics. The file is in WAL mode with a busy timeout, since two processes may open it (macOS has no single-instance lock under `cargo run`, and on Linux and Windows the lock can fail open).
- **Quitting** waits for the requests already queued, as the store flushes on quit, within GPUI's 200 ms limit for quit work (see the known limits below).
- **A closure that panics** returns `DataError::Panicked`, and its transaction is rolled back. The panic hook logs only where the panic happened, never its message, which may hold SQL text or values.
- **The key-value store stays as it is.** `store/<extension>.json` keeps its synchronous reads for small values; the database is for data that is queried.

## When something goes wrong, the user's data is kept

| What happens | What YARD does | Logged |
|---|---|---|
| The file cannot be opened or is not an SQLite database (corrupt, unreadable, a read-only disk at first open) | The file is not touched. Every request of this run returns `DataError::Unavailable`. | `ERROR` once in the diagnostic log; in the trace each refused request every time |
| The file has more migration steps for an extension than it declares (written by a newer version of the application) | That extension's requests return `DataError::Newer`; its tables are not touched. The other extensions work. | `WARN` once, naming the extension and both step counts |
| A recorded step's SQL differs from the declared one (a shipped step was edited) | That extension's requests return `DataError::Migration`. | `ERROR` once: a bug of the application |
| A migration step fails | Its transaction is rolled back, the step is not recorded, and the extension's requests return `DataError::Migration`. | `ERROR` once, with the step and SQLite's message |
| The file is locked by another process beyond the busy timeout | That request returns `DataError::Busy`; the next one tries again. | `WARN` |
| A statement on a table the extension does not own | Refused before it runs; the closure gets SQLite's authorization error. | `WARN`: a bug of the extension |
| Any other SQL error in a closure | Returned to the extension (`DataError::Sql`); YARD does not judge it. | `debug` |

| A closure panics | Its transaction is rolled back; the request returns `DataError::Panicked`. | `ERROR` with the panic's location only |

Nothing ever deletes, renames, or replaces `data.sqlite`. Unlike `state.json`, which YARD overwrites when it cannot read it, this file holds what the user made.

## Known limits

These are accepted for this slice. Each one that is deferred work has a `TODO` at its place and a YARD issue.

- **The authorizer guards against an extension's bugs, not against hostile code.** An extension is compiled into the application, and could open `data.sqlite` itself with rusqlite.
- **A read closure can write its own tables.** Nothing separates a read from a write except the transaction a write gets.
- **`sqlite_master` is readable.** An extension can read every table's `CREATE` statement, other extensions' included, but not their rows.
- **A view or trigger must come after the tables it names.** It is checked when it is created, so a view over a table that does not exist yet fails, where SQLite alone would accept it.
- **A trigger `UPDATE OF` only generated columns is refused.** It could never fire, and the check cannot see its body.
- **Quitting waits at most 200 ms** for the queued requests, as GPUI waits for quit work. A write queued behind another process's lock can be cut off. `TODO(data-store-quit-timeout)`, [YARD#3](https://github.com/Riso-platform-factory/YARD/issues/3).
- **Every request scans the whole schema** to check what the extension touches. It costs about 51 µs at 20 objects and 1.7 ms at 2,000, on the single database thread. `TODO(data-store-schema-cache)`, [YARD#4](https://github.com/Riso-platform-factory/YARD/issues/4).

## With extension-toggle

An extension turned off in the settings ([extension-toggle](./extension-toggle.md)) does not run `init`, so it declares no migrations and makes no requests: its tables, rows, and recorded steps stay as they are, and when it is turned on again its migrations continue from the last recorded step. YARD does not drop the tables of an extension that is no longer in the application's list.

## Logging

- `data-open{file}`: opening the file at the first request or migration, on the database thread: `debug` for WAL mode and the SQLite version.
- `data-migrate{extension, from, to}`: running an extension's new steps, `debug` per step. No group when there is nothing to run.
- `data-request{extension, kind}` (`kind` is `read` or `write`) at `trace`, since an application may make many: how long it waited and ran. The SQL text and the values are never logged, since they are the user's data.
- The `ERROR` and `WARN` lines of the table above, in the diagnostic log.

## Parts

- `yard_data/database`, in a new engine crate `crates/yard_data` (`src/dev/database.rs`): the database thread, opening the file, the migration runner, the authorizer, and `DataError`. It depends on `rusqlite` (features `bundled` and `hooks`) and on nothing of GPUI; it hands results back through channels.
- `yard_shell/data`, in `crates/yard_shell/src/dev/data.rs`: `set_data_file`, and `Extension::migrate_data`, `read_data`, `write_data`, which bind the extension's name to its requests and return GPUI tasks; the flush on quit. It adds the methods to `Extension` without changing the production part `yard_shell/shell`, as `yard_shell/file_dialog` did.
- `yard_paths/data_file`, in `crates/yard_paths/src/dev/data_file.rs`: `Paths::data_file()`, `<data>/data.sqlite`.
- The startup is a production part, and production code uses only production code, so `yard_app/startup` does not call `set_data_file` while the parts are under review: the review extension in `app/yard/src/dev/review.rs` does, first in its list. At promotion the call moves into the startup's `app-init`, beside `set_store_dir`, and the startup is shown to the user and stamped again.
- **Acceptance tests**, written before the code by a worker that does not write it, against a temporary file:
  - in `crates/yard_data/tests/`: migrations run once and in order across two opens; a second extension's steps are recorded apart; a newer file refuses only that extension and leaves its rows; an edited step refuses; a failing step rolls back and is not recorded; a write closure that returns `Err` or panics leaves nothing; the authorizer refuses another extension's table, a table without a prefix, and `"yard.migrations"`, and allows the extension's own; a file of random bytes gives `Unavailable` and is byte for byte the same afterwards; a second connection holding a write lock gives `Busy`;
  - in `crates/yard_shell/tests/`, with GPUI's test platform: requests run in order after the migrations, off the main thread; quitting waits for queued writes; an extension never reads another one's rows through the handle;
  - in their own files, the groups, fields, and levels above, and that no SQL text or value appears in either log.
- **Test extension** `examples/data_tour`, named in `review.rs` while the parts are under review. Without `DATA_TOUR` it does nothing. With `DATA_TOUR=on` it places a card listing its notes, with **Add note** (adds "Note <n>") and **Mark done** buttons, from two migration steps. The README walks through: add notes, quit, start again (the notes are back); `DATA_TOUR=old`, which declares only the first step (`WARN` newer, the card says the data is from a newer version, the file unchanged); `DATA_TOUR=peek`, which also reads the table of a second extension of the crate (refused, `WARN`); turning `data_tour` off and on in the settings (its notes come back); a file replaced by hand with random bytes (`ERROR`, the card says unavailable, the bytes unchanged); and two processes started with the same `YARD_HOME` on macOS (both work, and each sees the other's notes).
- **Run by the lead on macOS** with `cargo run dev` and every case. The Linux and Windows runners run the same cases from YARD-testing's `features/data-store/`, since they need only the trace, the file, and the screen.

## Not in this slice

- Reads in parallel with writes (a pool of read-only connections, as Zed has): one thread runs every request in order. `TODO(data-store-read-pool)`, [YARD#5](https://github.com/Riso-platform-factory/YARD/issues/5).
- Moving the key-value store, the node state, or the session into the database.
- Sharing tables between extensions, or declaring that one extension's tables depend on another's.
- Down migrations, backups, export, and encryption of the file (`secrets` keeps credentials, not data).
- A typed query builder or an ORM over the SQL.
