#![cfg(feature = "dev")]
//! A variable of the caller whose name starts with `XTASK_SIGN_`, even one
//! xtask does not know, reaches none of the commands of a signed bundle.
//!
//! It sets a variable of the process, so it is alone in its file.

#[allow(dead_code)]
mod support;

use std::path::Path;
use support::{FakeRunner, base64, tempdir, under_test};
use xtask::{Interrupt, WindowsArgs, WindowsHost, WindowsSecrets, bundle_windows};

fn host(temp: &Path) -> WindowsHost {
    let tools = temp.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for tool in ["ISCC.exe", "signtool.exe"] {
        std::fs::write(tools.join(tool), b"").unwrap();
    }
    let root = temp.join("Windows");
    let powershell = root.join("System32/WindowsPowerShell/v1.0");
    std::fs::create_dir_all(&powershell).unwrap();
    std::fs::write(powershell.join("powershell.exe"), b"").unwrap();
    for tool in ["tar.exe", "certutil.exe"] {
        std::fs::write(root.join("System32").join(tool), b"").unwrap();
    }
    WindowsHost {
        system: "windows".into(),
        path: Some(tools.into_os_string()),
        program_files_x86: None,
        system_root: Some(root.into_os_string()),
        xtask: Some(temp.join("xtask.exe")),
    }
}

#[test]
fn an_unknown_xtask_sign_variable_is_removed_from_every_command() {
    // SAFETY: the only test of this binary sets it before anything else
    // reads the environment.
    unsafe { std::env::set_var("XTASK_SIGN_SOMETHING_ELSE", "caller's") };
    let temp = tempdir();
    let mut runner = FakeRunner::new(temp.path());
    bundle_windows(
        &WindowsArgs {
            config: Some(under_test("signed")),
            out: Some(temp.path().join("out")),
            ..WindowsArgs::default()
        },
        &host(temp.path()),
        &WindowsSecrets {
            certificate: Some(base64(b"pfx")),
            certificate_password: Some("password".into()),
        },
        &Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert!(runner.commands.len() >= 10);
    for command in &runner.commands {
        assert!(
            command
                .envs
                .iter()
                .any(|(name, value)| name == "XTASK_SIGN_SOMETHING_ELSE" && value.is_none()),
            "removed from {command:?}"
        );
    }
}
