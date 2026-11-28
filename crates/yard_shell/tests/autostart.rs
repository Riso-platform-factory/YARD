use gpui::TestAppContext;
use yard_shell::{self as shell, AutostartBackend, AutostartError, AutostartStatus, Extension};

#[gpui::test]
fn memory_backend_reads_updates_and_repeats_idempotently(cx: &mut TestAppContext) {
    let backend = AutostartBackend::memory();
    let notes = cx.update(|cx| {
        shell::set_autostart_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled),
        "memory backend must start Disabled"
    );

    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(true, cx)),
        Ok(AutostartStatus::Enabled),
        "enabling must return Enabled"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Enabled),
        "reading after enable must return Enabled"
    );

    // Repeating enable succeeds with the same status.
    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(true, cx)),
        Ok(AutostartStatus::Enabled),
        "repeating enable must stay Ok(Enabled)"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Enabled)
    );

    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(false, cx)),
        Ok(AutostartStatus::Disabled),
        "disabling must return Disabled"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled),
        "reading after disable must return Disabled"
    );

    // Repeating disable succeeds with the same status.
    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(false, cx)),
        Ok(AutostartStatus::Disabled),
        "repeating disable must stay Ok(Disabled)"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled)
    );
}

#[gpui::test]
fn two_extensions_share_one_application_setting(cx: &mut TestAppContext) {
    let backend = AutostartBackend::memory();
    let (settings, tray) = cx.update(|cx| {
        shell::set_autostart_backend(cx, backend.clone());
        let settings = Extension::new(cx, "settings").unwrap();
        let tray = Extension::new(cx, "tray").unwrap();
        (settings, tray)
    });

    assert_eq!(
        cx.read(|cx| settings.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled)
    );
    assert_eq!(
        cx.read(|cx| tray.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled)
    );

    assert_eq!(
        cx.read(|cx| settings.set_launch_at_login(true, cx)),
        Ok(AutostartStatus::Enabled)
    );
    assert_eq!(
        cx.read(|cx| tray.launch_at_login(cx)),
        Ok(AutostartStatus::Enabled),
        "second extension must see Enabled set by the first"
    );

    assert_eq!(
        cx.read(|cx| tray.set_launch_at_login(false, cx)),
        Ok(AutostartStatus::Disabled)
    );
    assert_eq!(
        cx.read(|cx| settings.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled),
        "first extension must see Disabled set by the second"
    );
}

#[gpui::test]
fn set_status_drives_needs_approval_and_not_supported(cx: &mut TestAppContext) {
    let backend = AutostartBackend::memory();
    let notes = cx.update(|cx| {
        shell::set_autostart_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    backend.set_status(AutostartStatus::NeedsApproval);
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::NeedsApproval),
        "reading after set_status(NeedsApproval) must return NeedsApproval"
    );

    backend.set_status(AutostartStatus::NotSupported);
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::NotSupported),
        "reading after set_status(NotSupported) must return NotSupported"
    );
    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(true, cx)),
        Err(AutostartError::NotSupported),
        "enabling when NotSupported must return Err(NotSupported)"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::NotSupported)
    );
}

#[gpui::test]
fn fail_next_fails_once_and_recovers_on_next_call(cx: &mut TestAppContext) {
    let backend = AutostartBackend::memory();
    let notes = cx.update(|cx| {
        shell::set_autostart_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    backend.fail_next(AutostartError::Failed);
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Err(AutostartError::Failed),
        "fail_next(Failed) must make the next read return Err(Failed)"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled),
        "the read after fail_next must succeed"
    );

    backend.fail_next(AutostartError::Failed);
    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(true, cx)),
        Err(AutostartError::Failed),
        "fail_next(Failed) must make the next write return Err(Failed)"
    );
    assert_eq!(
        cx.read(|cx| notes.launch_at_login(cx)),
        Ok(AutostartStatus::Disabled),
        "a failed enable must leave the setting unchanged"
    );
    assert_eq!(
        cx.read(|cx| notes.set_launch_at_login(true, cx)),
        Ok(AutostartStatus::Enabled),
        "the write after fail_next must succeed"
    );
}

#[test]
fn error_display_texts_match_contract_and_hold_no_path() {
    let cases = [
        (
            AutostartError::NotSupported,
            "launch at login is not supported here",
        ),
        (
            AutostartError::NeedsApproval,
            "launch at login needs the user's approval in the system settings",
        ),
        (AutostartError::Failed, "launch at login failed"),
    ];

    for (error, expected) in cases {
        let _: &dyn std::error::Error = &error;
        let text = error.to_string();
        assert_eq!(text, expected);
        assert!(
            !text.contains('/') && !text.contains('\\'),
            "AutostartError Display text must never hold a path: {text:?}"
        );
    }
}
