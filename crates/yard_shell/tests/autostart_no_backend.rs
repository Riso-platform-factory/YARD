//! Before `set_autostart_backend` is called, both `launch_at_login` and
//! `set_launch_at_login` return `Err(AutostartError::Failed)` and log one
//! diagnostic `ERROR` per application.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use std::sync::Mutex;
use yard_shell::{AutostartError, Extension};

static DIAGNOSTICS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard_shell")
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

fn take_diagnostics() -> Vec<(log::Level, String)> {
    std::mem::take(&mut *DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn count_errors(records: &[(log::Level, String)]) -> usize {
    records
        .iter()
        .filter(|(level, _)| *level == log::Level::Error)
        .count()
}

#[gpui::test]
fn calls_without_backend_return_failed_with_one_error_per_app(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let first_app = cx.new_app();
    let second_app = cx.new_app();
    let first_notes = first_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let second_notes = second_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let _ = take_diagnostics();

    // First application: first call (read) logs one ERROR; follow-up calls log none.
    assert_eq!(
        first_app.read(|cx| first_notes.launch_at_login(cx)),
        Err(AutostartError::Failed),
        "first app launch_at_login without backend must return Failed"
    );
    let first_initial = take_diagnostics();
    assert_eq!(
        count_errors(&first_initial),
        1,
        "first call without backend in first app must log one ERROR: {first_initial:#?}"
    );

    assert_eq!(
        first_app.read(|cx| first_notes.set_launch_at_login(true, cx)),
        Err(AutostartError::Failed),
        "first app set_launch_at_login(true) without backend must return Failed"
    );
    assert_eq!(
        first_app.read(|cx| first_notes.set_launch_at_login(false, cx)),
        Err(AutostartError::Failed),
        "first app set_launch_at_login(false) without backend must return Failed"
    );
    assert_eq!(
        first_app.read(|cx| first_notes.launch_at_login(cx)),
        Err(AutostartError::Failed),
        "follow-up launch_at_login without backend in first app must return Failed"
    );
    let first_followup = take_diagnostics();
    assert_eq!(
        count_errors(&first_followup),
        0,
        "follow-up calls without backend in first app must log no ERROR: {first_followup:#?}"
    );

    // Second application: first call (write) logs one ERROR of its own; follow-up calls log none.
    assert_eq!(
        second_app.read(|cx| second_notes.set_launch_at_login(true, cx)),
        Err(AutostartError::Failed),
        "second app set_launch_at_login without backend must return Failed"
    );
    let second_initial = take_diagnostics();
    assert_eq!(
        count_errors(&second_initial),
        1,
        "first call without backend in second app must log one ERROR: {second_initial:#?}"
    );

    assert_eq!(
        second_app.read(|cx| second_notes.launch_at_login(cx)),
        Err(AutostartError::Failed),
        "second app launch_at_login without backend must return Failed"
    );
    assert_eq!(
        second_app.read(|cx| second_notes.set_launch_at_login(false, cx)),
        Err(AutostartError::Failed),
        "second app set_launch_at_login(false) without backend must return Failed"
    );
    let second_followup = take_diagnostics();
    assert_eq!(
        count_errors(&second_followup),
        0,
        "follow-up calls without backend in second app must log no ERROR: {second_followup:#?}"
    );

    first_app.quit();
    first_app.run_until_parked();
    second_app.quit();
    second_app.run_until_parked();
}
