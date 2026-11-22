#![cfg(feature = "dev")]
//! `cargo xtask sign-windows-file`, the real executable, run by hand without
//! what `bundle windows` gives it: it refuses at `info`, never as an error,
//! and exits 1.

#[allow(dead_code)]
mod support;

use std::process::Command;

#[test]
fn the_helper_without_its_context_refuses_and_exits_1() {
    let temp = support::tempdir();
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("sign-windows-file")
        .arg(temp.path().join("setup.exe"))
        .env("XTASK_LIFECYCLE", "info")
        .env("CARGO_TARGET_DIR", temp.path());
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("XTASK_SIGN_") {
            command.env_remove(name);
        }
    }
    let output = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let refusal =
        "XTASK_SIGN_THUMBPRINT is not set: sign-windows-file runs only from bundle windows";
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("INFO") && line.contains(&format!("refused: {refusal}"))),
        "the refusal at info in the trace: {stderr}"
    );
    assert!(
        !stderr.lines().any(|line| line.contains("ERROR")),
        "no error: {stderr}"
    );
    assert!(
        stderr.contains(&format!("sign-windows-file: {refusal}")),
        "{stderr}"
    );
    assert!(
        std::fs::read_dir(temp.path()).unwrap().next().is_none(),
        "the helper opens no log file"
    );
}
