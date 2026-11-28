//! Before `set_tray_backend` is called, `set_tray` returns `TrayError::Unavailable`
//! and logs one diagnostic `ERROR` per application.
//!
//! Installs a process-wide diagnostic logger; alone in its binary.

use gpui::TestAppContext;
use image::{ImageFormat, Rgba, RgbaImage};
use std::{io::Cursor, sync::Mutex};
use yard_shell::{Extension, Tray, TrayError, TrayIcon};

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

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("encoding test PNG must succeed");
    bytes
}

#[gpui::test]
fn set_tray_without_backend_returns_unavailable_with_one_error_per_app(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default()).unwrap();

    let icon = png_bytes(16, 16);

    let first_app = cx.new_app();
    let second_app = cx.new_app();
    let first_notes = first_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let second_notes = second_app.update(|cx| Extension::new(cx, "notes").unwrap());
    let _ = take_diagnostics();

    // First application: first call logs one ERROR; follow-up calls log none.
    assert_eq!(
        first_app.update(|cx| first_notes.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
        Err(TrayError::Unavailable),
        "first app set_tray without backend must return Unavailable"
    );
    let first_initial = take_diagnostics();
    assert_eq!(
        count_errors(&first_initial),
        1,
        "first set_tray without backend in first app must log one ERROR: {first_initial:#?}"
    );

    assert_eq!(
        first_app.update(|cx| first_notes.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
        Err(TrayError::Unavailable),
        "second set_tray without backend in first app must return Unavailable"
    );
    let first_followup = take_diagnostics();
    assert_eq!(
        count_errors(&first_followup),
        0,
        "follow-up set_tray without backend in first app must log no ERROR: {first_followup:#?}"
    );

    // Second application: first call logs one ERROR of its own; follow-up calls log none.
    assert_eq!(
        second_app.update(|cx| second_notes.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
        Err(TrayError::Unavailable),
        "second app set_tray without backend must return Unavailable"
    );
    let second_initial = take_diagnostics();
    assert_eq!(
        count_errors(&second_initial),
        1,
        "first set_tray without backend in second app must log one ERROR: {second_initial:#?}"
    );

    assert_eq!(
        second_app.update(|cx| second_notes.set_tray(Tray::new(TrayIcon::png(&icon)), cx)),
        Err(TrayError::Unavailable),
        "follow-up set_tray without backend in second app must return Unavailable"
    );
    let second_followup = take_diagnostics();
    assert_eq!(
        count_errors(&second_followup),
        0,
        "follow-up set_tray without backend in second app must log no ERROR: {second_followup:#?}"
    );

    first_app.quit();
    first_app.run_until_parked();
    second_app.quit();
    second_app.run_until_parked();
}
