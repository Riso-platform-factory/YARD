#![cfg(all(feature = "dev", target_os = "macos"))]
//! A temporary folder whose path `security -i` would read as two commands is
//! refused before any keychain is made.
//!
//! This test sets `TMPDIR` for its process, so it is alone in its file.

#[allow(dead_code)]
mod support;

use support::{FakeRunner, base64, signed_config, tempdir};
use xtask::{Interrupt, MacosArgs, Secrets, bundle_macos};

#[test]
fn a_tmpdir_with_a_line_break_is_refused() {
    let temp = tempdir();
    let tmpdir = temp.path().join("line\nbreak");
    std::fs::create_dir(&tmpdir).unwrap();
    // SAFETY: the only test of this binary sets it before it starts any
    // thread.
    unsafe { std::env::set_var("TMPDIR", &tmpdir) };
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_macos(
        &MacosArgs {
            config: Some(config),
            out: Some(temp.path().join("out")),
            ..MacosArgs::default()
        },
        &Secrets {
            certificate: Some(base64(b"certificate")),
            certificate_password: Some("password".into()),
            ..Secrets::default()
        },
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        (
            "keychain",
            "the keychain's path (in TMPDIR) holds a line break or a NUL, \
             which `security -i` cannot take"
        )
    );
    assert!(
        runner
            .commands
            .iter()
            .all(|command| command.program != "security"),
        "no keychain is made: {:?}",
        runner.programs()
    );
    assert_eq!(
        std::fs::read_dir(&tmpdir).unwrap().count(),
        0,
        "the private folder is removed"
    );
}
