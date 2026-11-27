use crate::Extension;
use gpui::{App, AppContext, Global, Task};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use yard_data::Database;

pub use yard_data::{DataError, Db, rusqlite};

struct AppDatabase(Database);

impl Global for AppDatabase {}

static REQUESTED_WITHOUT_FILE: AtomicBool = AtomicBool::new(false);

/// Gives the shell the application's data file. Called once, before any extension's request.
pub fn set_data_file(cx: &mut App, file: PathBuf) {
    if cx.try_global::<AppDatabase>().is_some() || REQUESTED_WITHOUT_FILE.load(Ordering::Relaxed) {
        log::error!("the data file must be set once before any extension data request");
        return;
    }
    let database = Database::new(file);
    cx.set_global(AppDatabase(database.clone()));
    cx.on_app_quit(move |cx| {
        let database = database.clone();
        // TODO(data-store-quit-timeout): GPUI waits at most 200 ms for on_app_quit, so a write
        // queued behind another process's lock can be cut off at quit. Unblocked by joining the
        // database thread after the application's run returns, in yard_app/startup.
        // See https://github.com/Riso-platform-factory/YARD/issues/3.
        cx.background_spawn(async move { database.flush() })
    })
    .detach();
}

fn database(cx: &App) -> Option<&Database> {
    if let Some(database) = cx.try_global::<AppDatabase>() {
        return Some(&database.0);
    }
    if !REQUESTED_WITHOUT_FILE.swap(true, Ordering::Relaxed) {
        log::error!(
            "extension data is unavailable because the application did not set a data file"
        );
    }
    tracing::trace!("extension data request refused because no data file was set");
    None
}

impl Extension {
    pub fn migrate_data(&self, cx: &mut App, steps: &'static [&'static str]) {
        if let Some(database) = database(cx) {
            drop(database.migrate(&self.name, steps));
        }
    }

    pub fn read_data<T: Send + 'static>(
        &self,
        cx: &App,
        f: impl FnOnce(&Db) -> rusqlite::Result<T> + Send + 'static,
    ) -> Task<Result<T, DataError>> {
        let Some(database) = database(cx) else {
            return Task::ready(Err(DataError::Unavailable));
        };
        cx.background_spawn(database.read(&self.name, f))
    }

    pub fn write_data<T: Send + 'static>(
        &self,
        cx: &App,
        f: impl FnOnce(&Db) -> rusqlite::Result<T> + Send + 'static,
    ) -> Task<Result<T, DataError>> {
        let Some(database) = database(cx) else {
            return Task::ready(Err(DataError::Unavailable));
        };
        cx.background_spawn(database.write(&self.name, f))
    }
}
