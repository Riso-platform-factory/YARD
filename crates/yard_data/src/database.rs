use futures::channel::oneshot;
use rusqlite::config::DbConfig;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, ErrorCode, Params, Row, Statement, TransactionBehavior};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Once};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

pub use rusqlite;

const BUSY_TIMEOUT: Duration = Duration::from_secs(1);

type Request = Box<dyn FnOnce(&mut DatabaseThread) + Send>;
type MigrationSteps = &'static [&'static str];

/// SQL access for one extension's request.
pub struct Db<'a> {
    connection: &'a Connection,
    extension: &'a str,
    access: &'a Mutex<Access>,
    schema: RefCell<Vec<SchemaObject>>,
}

impl Db<'_> {
    pub fn execute<P: Params>(&self, sql: &str, params: P) -> rusqlite::Result<usize> {
        let changed = self.prepare(sql)?.execute(params)?;
        self.validate_schema()?;
        Ok(changed)
    }

    pub fn execute_batch(&self, sql: &str) -> rusqlite::Result<()> {
        let mut batch = rusqlite::Batch::new(self.connection, sql);
        loop {
            self.start_preparation();
            let Some(mut statement) = self.check_preparation(batch.next())? else {
                return Ok(());
            };
            statement.raw_query().next()?;
            self.validate_schema()?;
        }
    }

    pub fn prepare(&self, sql: &str) -> rusqlite::Result<Statement<'_>> {
        self.start_preparation();
        self.check_preparation(self.connection.prepare(sql))
    }

    pub fn query_row<T, P: Params, F>(&self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    {
        let value = self.prepare(sql)?.query_row(params, f)?;
        self.validate_schema()?;
        Ok(value)
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.connection.last_insert_rowid()
    }

    fn start_preparation(&self) {
        let mut access = self.access.lock().unwrap();
        access.system_ddl = false;
        access.statistics_delete = false;
        access.function_refused = false;
    }

    fn check_preparation<T>(&self, result: rusqlite::Result<T>) -> rusqlite::Result<T> {
        let access = self.access.lock().unwrap();
        let direct_statistics_delete = access.statistics_delete && !access.system_ddl;
        let function_refused = access.function_refused;
        drop(access);
        match result {
            Ok(_) if direct_statistics_delete => Err(refuse(self.extension, "database")),
            // SQLite reports a function denial as SQLITE_ERROR during preparation.
            Err(_) if function_refused => Err(authorization_error()),
            Err(ref error) if schema_write_error(error) => Err(refuse(self.extension, "database")),
            result => result,
        }
    }

    fn validate_schema(&self) -> rusqlite::Result<()> {
        let mut access = self.access.lock().unwrap();
        if access.schema_refused {
            return Err(authorization_error());
        }
        if !access.schema_may_change {
            return Ok(());
        }
        let active = access.active;
        access.active = false;
        drop(access);

        let result = self.check_schema();

        let mut access = self.access.lock().unwrap();
        access.active = active;
        access.schema_refused = result.is_err();
        result
    }

    fn check_schema(&self) -> rusqlite::Result<()> {
        let before = self.schema.borrow();
        let after = schema_objects(self.connection)?;
        if let Some(owner) = schema_violation(self.connection, self.extension, &before, &after)? {
            return Err(refuse(self.extension, &owner));
        }
        for object in after.iter().filter(|object| !before.contains(object)) {
            match object.kind.as_str() {
                "view" => {
                    let view = qualified_name(&object.database, &object.name);
                    self.prepare_schema_probe(&format!("SELECT * FROM {view}"))?;
                }
                "trigger" => self.check_trigger(object, &after)?,
                _ => {}
            }
        }
        drop(before);
        self.schema.replace(after);
        Ok(())
    }

    fn prepare_schema_probe(&self, sql: &str) -> rusqlite::Result<()> {
        self.access.lock().unwrap().active = true;
        let result = self.prepare(sql).map(drop);
        self.access.lock().unwrap().active = false;
        result
    }

    fn check_trigger(
        &self,
        trigger: &SchemaObject,
        schema: &[SchemaObject],
    ) -> rusqlite::Result<()> {
        self.access.lock().unwrap().trigger_probe = Some((trigger.name.clone(), false));
        let result = self.prepare_trigger_probes(trigger, schema);
        let (_, seen) = self.access.lock().unwrap().trigger_probe.take().unwrap();
        result?;
        if !seen {
            return Err(refuse(self.extension, "database"));
        }
        Ok(())
    }

    fn prepare_trigger_probes(
        &self,
        trigger: &SchemaObject,
        schema: &[SchemaObject],
    ) -> rusqlite::Result<()> {
        // SQLite resolves trigger bodies when it prepares DML for the target.
        // TEMP triggers can be attached to a table or view in either schema.
        for target in schema.iter().filter(|object| {
            object.name.eq_ignore_ascii_case(&trigger.table)
                && matches!(object.kind.as_str(), "table" | "view")
                && (trigger.database == "temp" || object.database == trigger.database)
        }) {
            let name = qualified_name(&target.database, &target.name);
            let assignments = trigger_update_assignments(self.connection, target)?;
            for sql in [
                format!("EXPLAIN INSERT INTO {name} DEFAULT VALUES"),
                format!("EXPLAIN UPDATE {name} SET {assignments}"),
                format!("EXPLAIN DELETE FROM {name}"),
            ] {
                if let Err(error) = self.prepare_schema_probe(&sql)
                    && !is_view_without_trigger(target, &error)
                {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

/// A handle to the application's database thread. Clone, Send, Sync.
#[derive(Clone)]
pub struct Database {
    requests: Sender<Request>,
}

impl Database {
    /// Starts the database thread. The file is opened at the first request.
    pub fn new(file: PathBuf) -> Self {
        install_panic_hook();
        let (requests, queue) = mpsc::channel::<Request>();
        // TODO(data-store-read-pool): one thread runs every request in order, so reads wait behind
        // writes. Unblocked by read-only connections beside the writer, when an application's reads
        // wait behind its writes. See https://github.com/Riso-platform-factory/YARD/issues/5.
        if let Err(error) = thread::Builder::new()
            .name("yard-data".into())
            .spawn(move || {
                let mut database = DatabaseThread {
                    file,
                    connection: None,
                    pending_migrations: HashMap::new(),
                    refusals: HashMap::new(),
                };
                for request in queue {
                    request(&mut database);
                }
            })
        {
            log::error!("could not start the database thread: {error}");
            tracing::error!("could not start the database thread: {error}");
        }
        Self { requests }
    }

    /// Runs the extension's steps not yet recorded, in order.
    pub fn migrate(&self, extension: &str, steps: MigrationSteps) -> Reply<()> {
        self.request(extension, "migrate", Some(steps), |_, _| Ok(()))
    }

    pub fn read<T: Send + 'static>(
        &self,
        extension: &str,
        f: impl FnOnce(&Db) -> rusqlite::Result<T> + Send + 'static,
    ) -> Reply<T> {
        self.sql_request(extension, "read", TransactionBehavior::Deferred, f)
    }

    /// Runs the closure in a transaction, committing on success and rolling back on error or panic.
    pub fn write<T: Send + 'static>(
        &self,
        extension: &str,
        f: impl FnOnce(&Db) -> rusqlite::Result<T> + Send + 'static,
    ) -> Reply<T> {
        self.sql_request(extension, "write", TransactionBehavior::Immediate, f)
    }

    fn sql_request<T: Send + 'static>(
        &self,
        extension: &str,
        kind: &'static str,
        behavior: TransactionBehavior,
        f: impl FnOnce(&Db) -> rusqlite::Result<T> + Send + 'static,
    ) -> Reply<T> {
        self.request(extension, kind, None, move |connection, extension| {
            with_authorizer(connection, extension, |connection, access| {
                let transaction = connection
                    .transaction_with_behavior(behavior)
                    .map_err(sql_error)?;
                let value = run_extension(&transaction, extension, access, f)?;
                transaction.commit().map_err(sql_error)?;
                Ok(value)
            })
        })
    }

    /// Blocks until every request sent before it has run.
    pub fn flush(&self) {
        let _ = self.enqueue(|_| Ok(())).wait();
    }

    fn request<T: Send + 'static>(
        &self,
        extension: &str,
        kind: &'static str,
        steps: Option<MigrationSteps>,
        run: impl FnOnce(&mut Connection, &str) -> Result<T, DataError> + Send + 'static,
    ) -> Reply<T> {
        let queued = Instant::now();
        let extension = extension.to_owned();
        self.enqueue(move |database| {
            let started = Instant::now();
            let _span = tracing::trace_span!(
                "data-request",
                extension,
                kind,
                wait_us = started.duration_since(queued).as_micros() as u64
            )
            .entered();
            let result = database.run(&extension, steps, run);
            tracing::trace!(
                run_us = started.elapsed().as_micros() as u64,
                "request finished"
            );
            result
        })
    }

    fn enqueue<T: Send + 'static>(
        &self,
        run: impl FnOnce(&mut DatabaseThread) -> Result<T, DataError> + Send + 'static,
    ) -> Reply<T> {
        let (answer, receiver) = oneshot::channel();
        let _ = self.requests.send(Box::new(move |database| {
            let _ = answer.send(run(database));
        }));
        Reply { receiver }
    }
}

/// The answer to one request.
pub struct Reply<T> {
    receiver: oneshot::Receiver<Result<T, DataError>>,
}

impl<T> Reply<T> {
    /// Blocks the calling thread until the answer comes.
    pub fn wait(self) -> Result<T, DataError> {
        futures::executor::block_on(self)
    }
}

impl<T> Future for Reply<T> {
    type Output = Result<T, DataError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver)
            .poll(cx)
            .map(|answer| answer.unwrap_or(Err(DataError::Unavailable)))
    }
}

#[derive(Debug)]
pub enum DataError {
    /// The file cannot be opened or is not an SQLite database, or no file was set.
    Unavailable,
    /// The file records more steps for this extension than it declares.
    Newer { declared: usize, recorded: usize },
    /// A recorded step differs from the declared one, or a step failed.
    Migration,
    /// Another process held the lock beyond the busy timeout.
    Busy,
    /// The closure panicked; its transaction was rolled back.
    Panicked,
    /// Any other SQLite error, including the authorizer's refusal.
    Sql(rusqlite::Error),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("the database is unavailable"),
            Self::Newer { declared, recorded } => {
                write!(
                    f,
                    "the database has {recorded} migration steps, but only {declared} are declared"
                )
            }
            Self::Migration => f.write_str("a database migration failed"),
            Self::Busy => f.write_str("the database is busy"),
            Self::Panicked => f.write_str("the database request panicked"),
            Self::Sql(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sql(error) => Some(error),
            _ => None,
        }
    }
}

struct DatabaseThread {
    file: PathBuf,
    connection: Option<Result<Connection, ()>>,
    pending_migrations: HashMap<String, MigrationSteps>,
    refusals: HashMap<String, MigrationRefusal>,
}

#[derive(Clone, Copy)]
enum MigrationRefusal {
    Newer { declared: usize, recorded: usize },
    Invalid,
}

impl From<MigrationRefusal> for DataError {
    fn from(refusal: MigrationRefusal) -> Self {
        match refusal {
            MigrationRefusal::Newer { declared, recorded } => Self::Newer { declared, recorded },
            MigrationRefusal::Invalid => Self::Migration,
        }
    }
}

impl DatabaseThread {
    fn run<T>(
        &mut self,
        extension: &str,
        steps: Option<MigrationSteps>,
        run: impl FnOnce(&mut Connection, &str) -> Result<T, DataError>,
    ) -> Result<T, DataError> {
        if let Some(&refusal) = self.refusals.get(extension) {
            tracing::info!(reason = %DataError::from(refusal), "extension's migration was refused");
            return Err(refusal.into());
        }

        if let Some(steps) = steps {
            self.pending_migrations.insert(extension.to_owned(), steps);
        }
        if self.connection.is_none() {
            match open(&self.file) {
                Ok(connection) => self.connection = Some(Ok(connection)),
                Err(DataError::Busy) => return Err(DataError::Busy),
                Err(_) => self.connection = Some(Err(())),
            }
        }
        let Some(Ok(connection)) = &mut self.connection else {
            tracing::info!("request refused because the database is unavailable");
            return Err(DataError::Unavailable);
        };

        if let Some(&steps) = self.pending_migrations.get(extension) {
            let result = migrate(connection, extension, steps);
            match &result {
                Ok(()) => {
                    self.pending_migrations.remove(extension);
                }
                Err(DataError::Newer { declared, recorded }) => {
                    self.refusals.insert(
                        extension.to_owned(),
                        MigrationRefusal::Newer {
                            declared: *declared,
                            recorded: *recorded,
                        },
                    );
                }
                Err(DataError::Migration) => {
                    self.refusals
                        .insert(extension.to_owned(), MigrationRefusal::Invalid);
                }
                Err(_) => {}
            }
            result?;
        }
        run(connection, extension)
    }
}

fn open(file: &Path) -> Result<Connection, DataError> {
    let _span = tracing::info_span!("data-open", file = %file.display()).entered();
    if let Some(parent) = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log::error!("could not open {}: {error}", file.display());
        tracing::error!(%error, "could not create the database directory");
        return Err(DataError::Unavailable);
    }
    let setup = || {
        let connection = Connection::open(file)?;
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(()))?;
        let journal_mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if journal_mode != "wal" {
            return Err(rusqlite::Error::InvalidQuery);
        }
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL")?;
        connection.execute_batch(
            r#"CREATE TABLE IF NOT EXISTS "yard.migrations" (
                extension TEXT NOT NULL,
                step INTEGER NOT NULL,
                sql TEXT NOT NULL,
                PRIMARY KEY (extension, step)
            )"#,
        )?;
        tracing::debug!(
            journal_mode,
            sqlite = rusqlite::version(),
            "opened database"
        );
        Ok(connection)
    };
    match setup() {
        Ok(connection) => Ok(connection),
        Err(error) if is_busy(&error) => Err(sql_error(error)),
        Err(error) => {
            let reason = sqlite_message(&error);
            log::error!("could not open {}: {reason}", file.display());
            tracing::error!(reason, "could not open database");
            Err(DataError::Unavailable)
        }
    }
}

fn migrate(connection: &mut Connection, extension: &str, steps: &[&str]) -> Result<(), DataError> {
    with_authorizer(connection, extension, |connection, access| {
        let mut group = None;
        loop {
            // Hold the writer lock from the ledger check to recording each step so
            // two processes cannot both apply the same migration.
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let recorded = recorded_steps(&transaction, extension).map_err(sql_error)?;
            let from = recorded.len();
            let to = steps.len();
            if from > to {
                log::warn!("{extension} declares {to} migration steps, but {from} are recorded");
                tracing::warn!(
                    declared = to,
                    recorded = from,
                    "database migrations are newer"
                );
                return Err(DataError::Newer {
                    declared: to,
                    recorded: from,
                });
            }
            if let Some(step) = recorded.iter().zip(steps).enumerate().position(
                |(index, ((recorded_index, sql), declared))| {
                    *recorded_index != index as i64 || sql != declared
                },
            ) {
                log::error!(
                    "{extension} migration step {} differs from its recorded step",
                    step + 1
                );
                tracing::error!(step = step + 1, "migration differs from the recorded step");
                return Err(DataError::Migration);
            }
            let Some(sql) = steps.get(from) else {
                return Ok(());
            };

            group.get_or_insert_with(|| {
                tracing::info_span!("data-migrate", extension, from, to).entered()
            });
            let step = from + 1;
            tracing::debug!(step, "running migration step");
            let result = run_extension(&transaction, extension, access, |db| db.execute_batch(sql))
                .and_then(|()| {
                    transaction
                        .execute(
                            r#"INSERT INTO "yard.migrations" (extension, step, sql) VALUES (?1, ?2, ?3)"#,
                            (extension, from as i64, sql),
                        )
                        .map_err(DataError::Sql)?;
                    transaction.commit().map_err(DataError::Sql)
                });
            if let Err(error) = result {
                if matches!(error, DataError::Busy) {
                    return Err(error);
                }
                if let DataError::Sql(error) = &error
                    && is_busy(error)
                {
                    return Err(sql_error_from_busy());
                }
                let reason = match &error {
                    DataError::Sql(error) => sqlite_message(error),
                    _ => "migration did not complete",
                };
                log::error!("{extension} migration step {step} failed: {reason}");
                tracing::error!(step, reason, "migration step failed");
                return Err(DataError::Migration);
            }
        }
    })
}

fn recorded_steps(
    connection: &Connection,
    extension: &str,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut query = connection
        .prepare(r#"SELECT step, sql FROM "yard.migrations" WHERE extension = ?1 ORDER BY step"#)?;
    query
        .query_map([extension], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

#[derive(Default)]
struct Access {
    active: bool,
    schema_may_change: bool,
    schema_refused: bool,
    system_ddl: bool,
    statistics_delete: bool,
    function_refused: bool,
    blocked_tables: HashSet<(String, String)>,
    trigger_probe: Option<(String, bool)>,
}

fn with_authorizer<T>(
    connection: &mut Connection,
    extension: &str,
    run: impl FnOnce(&mut Connection, &Mutex<Access>) -> Result<T, DataError>,
) -> Result<T, DataError> {
    let access = Arc::new(Mutex::new(Access::default()));
    let scope = Arc::clone(&access);
    let extension = extension.to_owned();
    connection
        .authorizer(Some(move |context: AuthContext<'_>| {
            let mut access = scope.lock().unwrap();
            if let Some((name, seen)) = &mut access.trigger_probe
                && context.accessor == Some(name)
            {
                *seen = true;
            }
            if access.active
                && let Some(owner) = denied_owner(&extension, context, &mut access)
            {
                drop(access);
                refuse(&extension, owner);
                return Authorization::Deny;
            }
            Authorization::Allow
        }))
        .map_err(sql_error)?;
    // ON CONFLICT ROLLBACK can end the engine's transaction. If a closure
    // handles that error, its later statements still cannot commit on their own.
    let scope = Arc::clone(&access);
    connection
        .commit_hook(Some(move || scope.lock().unwrap().active))
        .map_err(sql_error)?;
    let result = run(connection, &access);
    connection
        .commit_hook(None::<fn() -> bool>)
        .map_err(sql_error)?;
    connection
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .map_err(sql_error)?;
    result
}

thread_local! {
    static IN_DATABASE_REQUEST: Cell<bool> = const { Cell::new(false) };
}

fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            if IN_DATABASE_REQUEST.get() {
                if let Some(location) = panic.location() {
                    log::error!("database request panicked at {location}");
                    tracing::error!(%location, "database request panicked");
                } else {
                    log::error!("database request panicked");
                    tracing::error!("database request panicked");
                }
            } else {
                previous(panic);
            }
        }));
    });
}

fn run_extension<T>(
    connection: &Connection,
    extension: &str,
    access: &Mutex<Access>,
    run: impl FnOnce(&Db) -> rusqlite::Result<T>,
) -> Result<T, DataError> {
    // TODO(data-store-schema-cache): every request scans sqlite_schema and pragma_table_list,
    // which grows with the schema and runs on the single database thread. Unblocked by caching
    // the snapshot keyed by PRAGMA schema_version. See https://github.com/Riso-platform-factory/YARD/issues/4.
    let db = Db {
        connection,
        extension,
        access,
        schema: RefCell::new(schema_objects(connection).map_err(sql_error)?),
    };
    let blocked_tables = blocked_tables(connection).map_err(sql_error)?;
    *access.lock().unwrap() = Access {
        active: true,
        blocked_tables,
        ..Access::default()
    };
    // SAFETY: This thread exclusively owns the open connection for the request.
    // Resetting the SQLite register covers both SQL and the Db accessor, including
    // inserts executed through a returned Statement.
    unsafe { rusqlite::ffi::sqlite3_set_last_insert_rowid(connection.handle(), 0) };
    IN_DATABASE_REQUEST.set(true);
    let result = catch_unwind(AssertUnwindSafe(|| run(&db))).map_err(|payload| {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
            // A panic payload can itself have a panicking destructor.
            std::mem::forget(payload);
        }
    });
    IN_DATABASE_REQUEST.set(false);
    access.lock().unwrap().active = false;
    match result {
        Ok(result) => {
            let value = result.map_err(sql_error)?;
            db.validate_schema().map_err(sql_error)?;
            Ok(value)
        }
        Err(_) => Err(DataError::Panicked),
    }
}

fn denied_owner<'a>(
    extension: &str,
    context: AuthContext<'a>,
    access: &mut Access,
) -> Option<&'a str> {
    use AuthAction::*;
    match context.action {
        AlterTable { .. }
        | Analyze { .. }
        | DropTable { .. }
        | DropTempTable { .. }
        | DropIndex { .. }
        | DropTempIndex { .. } => {
            access.system_ddl = context.accessor.is_none();
        }
        _ => {}
    }
    match context.action {
        Select | Recursive => None,
        Function { function_name } if function_name.eq_ignore_ascii_case("load_extension") => {
            Some("database")
        }
        Function { function_name }
            if function_name.eq_ignore_ascii_case("changes")
                || function_name.eq_ignore_ascii_case("total_changes") =>
        {
            access.function_refused = true;
            Some("database")
        }
        Function { .. } => None,
        CreateIndex {
            index_name,
            table_name,
        }
        | CreateTempIndex {
            index_name,
            table_name,
        }
        | DropIndex {
            index_name,
            table_name,
        }
        | DropTempIndex {
            index_name,
            table_name,
        } => unowned_object(extension, table_name).or_else(|| {
            if index_name.starts_with("sqlite_autoindex_") {
                None
            } else {
                unowned_object(extension, index_name)
            }
        }),
        CreateTrigger {
            trigger_name,
            table_name,
        }
        | CreateTempTrigger {
            trigger_name,
            table_name,
        }
        | DropTrigger {
            trigger_name,
            table_name,
        }
        | DropTempTrigger {
            trigger_name,
            table_name,
        } => unowned_object(extension, table_name)
            .or_else(|| unowned_object(extension, trigger_name)),
        CreateView { view_name }
        | CreateTempView { view_name }
        | DropView { view_name }
        | DropTempView { view_name } => unowned_object(extension, view_name),
        Insert { table_name } | Delete { table_name } | Update { table_name, .. }
            if is_schema_table(table_name) =>
        {
            // Defensive mode rejects direct edits to these tables. SQLite
            // uses the same authorizer actions for its own catalog updates.
            access.schema_may_change = true;
            None
        }
        Read { table_name, .. } if is_schema_table(table_name) => None,
        CreateTable { table_name } | CreateTempTable { table_name }
            if is_internal_table(table_name) =>
        {
            None
        }
        Delete { table_name } if is_statistics_table(table_name) && context.accessor.is_none() => {
            // ANALYZE can clear old statistics before SQLite reports its
            // target table. Check the completed preparation before stepping.
            access.statistics_delete = true;
            None
        }
        Read { table_name, .. } | Update { table_name, .. } | Delete { table_name }
            if is_internal_table(table_name)
                && context.accessor.is_none()
                && (access.system_ddl || access.statistics_delete) =>
        {
            None
        }
        Read { table_name, .. }
            if matches!(table_name, "json_each" | "json_tree")
                && !is_blocked_table(access, context.database_name, table_name) =>
        {
            None
        }
        AlterTable {
            database_name,
            table_name,
        } if is_blocked_table(access, Some(database_name), table_name) => Some("database"),
        Read { table_name, .. }
        | Insert { table_name }
        | Delete { table_name }
        | Update { table_name, .. }
            if is_blocked_table(access, context.database_name, table_name) =>
        {
            Some("database")
        }
        CreateTable { table_name }
        | CreateTempTable { table_name }
        | DropTable { table_name }
        | DropTempTable { table_name }
        | Insert { table_name }
        | Delete { table_name }
        | Update { table_name, .. }
        | Read { table_name, .. }
        | AlterTable { table_name, .. }
        | Analyze { table_name } => unowned_object(extension, table_name),
        Reindex { index_name } => unowned_object(extension, index_name),
        // The engine controls transaction boundaries. Connection-wide operations
        // and virtual-table modules can act on data outside the extension's tables.
        _ => Some("database"),
    }
}

fn is_blocked_table(access: &Access, database: Option<&str>, table: &str) -> bool {
    !access.blocked_tables.is_empty()
        && access
            .blocked_tables
            .contains(&(database.unwrap_or("main").to_owned(), table.to_owned()))
}

fn blocked_tables(connection: &Connection) -> rusqlite::Result<HashSet<(String, String)>> {
    connection
        .prepare(
            "SELECT schema, name FROM pragma_table_list
             WHERE type IN ('virtual', 'shadow') OR lower(name) IN ('json_each', 'json_tree')",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

fn is_schema_table(name: &str) -> bool {
    matches!(
        name,
        "sqlite_master" | "sqlite_schema" | "sqlite_temp_master" | "sqlite_temp_schema"
    )
}

fn is_statistics_table(name: &str) -> bool {
    matches!(name, "sqlite_stat1" | "sqlite_stat4")
}

fn is_internal_table(name: &str) -> bool {
    name == "sqlite_sequence" || is_statistics_table(name)
}

#[derive(PartialEq)]
struct SchemaObject {
    database: String,
    kind: String,
    name: String,
    table: String,
    sql: Option<String>,
}

impl SchemaObject {
    fn unowned_by(&self, extension: &str) -> bool {
        let name = if self.kind == "index" && self.name.starts_with("sqlite_autoindex_") {
            &self.table
        } else {
            &self.name
        };
        unowned_object(extension, name).is_some()
    }
}

fn schema_objects(connection: &Connection) -> rusqlite::Result<Vec<SchemaObject>> {
    connection
        .prepare(
            "SELECT 'main', type, name, tbl_name, sql FROM main.sqlite_schema
             UNION ALL
             SELECT 'temp', type, name, tbl_name, sql FROM temp.sqlite_schema
             ORDER BY 1, 2, 3",
        )?
        .query_map([], |row| {
            Ok(SchemaObject {
                database: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                table: row.get(3)?,
                sql: row.get(4)?,
            })
        })?
        .filter(|row| {
            !row.as_ref()
                .is_ok_and(|object| is_internal_table(&object.name))
        })
        .collect()
}

fn schema_violation(
    connection: &Connection,
    extension: &str,
    before: &[SchemaObject],
    after: &[SchemaObject],
) -> rusqlite::Result<Option<String>> {
    // This also catches another extension's view or trigger definition being
    // rewritten as a side effect of a rename.
    if !before
        .iter()
        .filter(|object| object.unowned_by(extension))
        .eq(after.iter().filter(|object| object.unowned_by(extension)))
    {
        return Ok(Some("database".to_owned()));
    }

    // The authorizer does not report the parent named in REFERENCES. Ask
    // SQLite for the parsed definitions of new or altered tables.
    let mut foreign_keys =
        connection.prepare(r#"SELECT "table" FROM pragma_foreign_key_list(?1, ?2)"#)?;
    for table in after.iter().filter(|object| {
        object.kind == "table" && !object.unowned_by(extension) && !before.contains(object)
    }) {
        let parents = foreign_keys.query_map((&table.name, &table.database), |row| {
            row.get::<_, String>(0)
        })?;
        for parent in parents {
            let parent = parent?;
            let resolved = after.iter().find(|object| {
                object.database == table.database
                    && object.kind == "table"
                    && object.name.eq_ignore_ascii_case(&parent)
            });
            if let Some(owner) = unowned_object(extension, &parent)
                .or_else(|| resolved.and_then(|object| unowned_object(extension, &object.name)))
            {
                return Ok(Some(owner.to_owned()));
            }
        }
    }
    Ok(None)
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn qualified_name(database: &str, name: &str) -> String {
    format!("{}.{}", quote_identifier(database), quote_identifier(name))
}

fn trigger_update_assignments(
    connection: &Connection,
    target: &SchemaObject,
) -> rusqlite::Result<String> {
    let mut columns =
        connection.prepare("SELECT name FROM pragma_table_xinfo(?1, ?2) WHERE hidden = 0")?;
    let mut columns: Vec<String> = columns
        .query_map((&target.name, &target.database), |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let has_rowid: bool = connection.query_row(
        "SELECT type = 'table' AND wr = 0 FROM pragma_table_list WHERE schema = ?1 AND name = ?2",
        (&target.database, &target.name),
        |row| row.get(0),
    )?;
    if has_rowid {
        for alias in ["rowid", "oid", "_rowid_"] {
            if !columns
                .iter()
                .any(|column| column.eq_ignore_ascii_case(alias))
            {
                columns.push(alias.to_owned());
            }
        }
    }
    Ok(columns
        .iter()
        .map(|column| format!("{} = NULL", quote_identifier(column)))
        .collect::<Vec<_>>()
        .join(", "))
}

fn is_view_without_trigger(target: &SchemaObject, error: &rusqlite::Error) -> bool {
    if target.kind != "view" {
        return false;
    }
    let message = match error {
        rusqlite::Error::SqlInputError { msg, .. }
        | rusqlite::Error::SqliteFailure(_, Some(msg)) => msg,
        _ => return false,
    };
    message == &format!("cannot modify {} because it is a view", target.name)
}

fn schema_write_error(error: &rusqlite::Error) -> bool {
    let message = match error {
        rusqlite::Error::SqlInputError { msg, .. }
        | rusqlite::Error::SqliteFailure(_, Some(msg)) => msg,
        _ => return false,
    };
    message
        .strip_prefix("table ")
        .and_then(|name| name.strip_suffix(" may not be modified"))
        .is_some_and(is_schema_table)
}

fn authorization_error() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_AUTH), None)
}

fn refuse(extension: &str, owner: &str) -> rusqlite::Error {
    log::warn!("database access refused for {extension}, owner {owner}");
    tracing::warn!(extension, owner, "database access refused");
    authorization_error()
}

fn unowned_object<'a>(extension: &str, name: &'a str) -> Option<&'a str> {
    match name.split_once('.') {
        Some((owner, _)) if owner == extension && !name.eq_ignore_ascii_case("yard.migrations") => {
            None
        }
        Some((owner, _)) => Some(owner),
        None => Some("unowned"),
    }
}

fn is_busy(error: &rusqlite::Error) -> bool {
    error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy)
}

fn sql_error(error: rusqlite::Error) -> DataError {
    if is_busy(&error) {
        return sql_error_from_busy();
    }
    if error.sqlite_error_code() != Some(ErrorCode::AuthorizationForStatementDenied) {
        tracing::debug!(reason = sqlite_message(&error), "SQLite request failed");
    }
    DataError::Sql(error)
}

fn sql_error_from_busy() -> DataError {
    log::warn!("the database is busy");
    tracing::warn!("the database is busy");
    DataError::Busy
}

fn sqlite_message(error: &rusqlite::Error) -> &'static str {
    // SQLite's detailed messages can contain values, identifiers, or SQL text.
    match error {
        rusqlite::Error::SqlInputError { error, .. } | rusqlite::Error::SqliteFailure(error, _) => {
            rusqlite::ffi::code_to_str(error.extended_code)
        }
        _ => "SQLite operation failed",
    }
}
