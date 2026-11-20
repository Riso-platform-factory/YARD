#![cfg(feature = "dev")]
//! The diagnostic log of `yard_log`, as `xtask::main` starts it: a failed
//! run writes its `ERROR` line to `<target>/xtask.log`, and its steps, logged
//! with `tracing`, stay out of it.
//!
//! `yard_log::start` installs a process-wide `log` logger, so this test is
//! alone in its file.

#[allow(dead_code)]
mod support;

use support::{FakeRunner, tempdir, under_test};
use xtask::{Interrupt, MacosArgs, bundle_macos};

#[test]
fn a_failed_run_writes_its_error_to_the_diagnostic_log() {
    let temp = tempdir();
    let target = temp.path().join("target");
    yard_log::start(yard_product::Product::new(
        "xtask",
        "0.1.0",
        "dev.yard.xtask",
    ));
    yard_log::open_files(&target);

    let options = MacosArgs {
        config: Some(under_test("plain")),
        out: Some(temp.path().join("out")),
        ..MacosArgs::default()
    };
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("codesign");
    bundle_macos(
        &options,
        &xtask::Secrets::default(),
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    log::logger().flush();

    let log = std::fs::read_to_string(target.join("xtask.log")).unwrap();
    let failed: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("failed at sign: codesign exited with exit status: 1"))
        .collect();
    assert_eq!(failed.len(), 1, "{log}");
    assert!(failed[0].contains("ERROR"), "{log}");
    assert!(
        !log.lines().any(|line| line.ends_with("stages")),
        "the steps are not in the diagnostic log: {log}"
    );
    assert!(!temp.path().join("out").join("xtask.log").exists());
}
