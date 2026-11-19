// This separate binary owns the diagnostic logger. It exercises the exact
// shared shape/paint failure policy without a public error-injection API.
#[path = "../src/text_input/failure.rs"]
mod failure;

use std::sync::Mutex;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LINES
            .lock()
            .unwrap()
            .push(format!("{} {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

#[test]
fn shape_and_paint_share_one_failure_diagnostic_until_the_text_changes() {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let input = failure::Failure::default();
    input.report("shape", "invalid glyph run");
    input.report("shape", "invalid glyph run");
    input.report("paint", "missing glyph");
    assert_eq!(
        *LINES.lock().unwrap(),
        ["ERROR could not shape text input: invalid glyph run"]
    );
    input.changed();
    input.report("paint", "missing glyph");
    input.report("paint", "missing glyph");
    let another_input = failure::Failure::default();
    another_input.report("shape", "another failure");
    assert_eq!(
        *LINES.lock().unwrap(),
        [
            "ERROR could not shape text input: invalid glyph run",
            "ERROR could not paint text input: missing glyph",
            "ERROR could not shape text input: another failure",
        ]
    );
}
