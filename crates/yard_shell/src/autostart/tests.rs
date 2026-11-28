use super::{AutostartError, AutostartStatus};
use std::{fs, path::Path};

#[test]
fn desktop_entry_contains_required_keys_and_quoted_exec() {
    let exe = Path::new("/opt/My App/bin/my-app");
    let entry = super::desktop_entry("My App", exe);
    let lines: Vec<&str> = entry.lines().collect();

    assert!(
        lines.contains(&"[Desktop Entry]"),
        "missing [Desktop Entry] header in:\n{entry}"
    );
    assert!(
        lines.contains(&"Type=Application"),
        "missing Type=Application in:\n{entry}"
    );
    assert!(
        lines.contains(&"Name=My App"),
        "missing Name=My App in:\n{entry}"
    );
    assert!(
        lines.contains(&"Exec=\"/opt/My App/bin/my-app\""),
        "missing quoted Exec line in:\n{entry}"
    );
    assert!(
        lines.contains(&"X-GNOME-Autostart-enabled=true"),
        "missing X-GNOME-Autostart-enabled=true in:\n{entry}"
    );
    assert!(
        super::desktop_entry_enabled(&entry),
        "generated desktop entry must read as enabled"
    );
}

#[test]
fn desktop_entry_enabled_respects_hidden_gnome_flag_structure_and_bom() {
    let generated = super::desktop_entry("My App", Path::new("/opt/My App/bin/my-app"));
    assert!(super::desktop_entry_enabled(&generated));

    let hidden = format!("{generated}\nHidden=true\n");
    assert!(
        !super::desktop_entry_enabled(&hidden),
        "Hidden=true must read as disabled"
    );

    let gnome_disabled = generated.replace(
        "X-GNOME-Autostart-enabled=true",
        "X-GNOME-Autostart-enabled=false",
    );
    assert!(
        !super::desktop_entry_enabled(&gnome_disabled),
        "X-GNOME-Autostart-enabled=false must read as disabled"
    );

    let minimal = "[Desktop Entry]\nType=Application\nName=My App\nExec=\"/usr/bin/my-app\"\n";
    assert!(
        super::desktop_entry_enabled(minimal),
        "entry without disabling keys must read as enabled"
    );

    let with_bom = format!("\u{feff}{generated}");
    assert!(
        super::desktop_entry_enabled(&with_bom),
        "generated entry with leading UTF-8 BOM must read as enabled"
    );

    assert!(
        !super::desktop_entry_enabled(""),
        "empty desktop entry must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled("Type=Application\nName=My App\nExec=\"/usr/bin/my-app\"\n"),
        "entry without [Desktop Entry] group must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled("[Desktop Entry]\nName=My App\nExec=\"/usr/bin/my-app\"\n"),
        "entry without Type=Application must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled(
            "[Desktop Entry]\nType=Link\nName=My App\nExec=\"/usr/bin/my-app\"\n"
        ),
        "entry with non-Application Type must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled("[Desktop Entry]\nType=Application\nName=My App\n"),
        "entry without Exec= must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled(
            "[Desktop Action x]\nType=Application\nName=My App\nExec=\"/usr/bin/my-app\"\n"
        ),
        "keys only in another group must read as disabled"
    );
    assert!(
        !super::desktop_entry_enabled(
            "[Desktop Entry]\nName=My App\n[Desktop Action x]\nType=Application\nExec=\"/usr/bin/my-app\"\n"
        ),
        "Type and Exec placed in another group must read as disabled"
    );
}

fn valid_desktop_entry() -> String {
    let entry = super::desktop_entry("My App", Path::new("/opt/My App/bin/my-app"));
    assert!(
        super::desktop_entry_enabled(&entry),
        "baseline desktop entry must read as enabled"
    );
    entry
}

#[test]
fn desktop_entry_enabled_rejects_empty_exec() {
    let valid = valid_desktop_entry();
    let entry = valid.replace("Exec=\"/opt/My App/bin/my-app\"", "Exec=");
    assert_ne!(entry, valid);
    assert!(
        !super::desktop_entry_enabled(&entry),
        "Exec= with an empty value must read as disabled"
    );
}

#[test]
fn desktop_entry_enabled_rejects_unclosed_exec_quote() {
    let valid = valid_desktop_entry();
    let entry = valid.replace("Exec=\"/opt/My App/bin/my-app\"", "Exec=\"/opt/app");
    assert_ne!(entry, valid);
    assert!(
        !super::desktop_entry_enabled(&entry),
        "Exec with an unclosed quote must read as disabled"
    );
}

#[test]
fn desktop_entry_enabled_rejects_space_in_key_name() {
    let entry = format!("{}Bad Key=1\n", valid_desktop_entry());
    assert!(
        !super::desktop_entry_enabled(&entry),
        "key name containing a space must read as disabled"
    );
}

#[test]
fn desktop_entry_enabled_rejects_non_key_value_line() {
    let entry = format!("{}not a key value line\n", valid_desktop_entry());
    assert!(
        !super::desktop_entry_enabled(&entry),
        "line without '=' must read as disabled"
    );
}

#[test]
fn desktop_entry_enabled_accepts_comments_blanks_locales_key_chars_and_other_groups() {
    let generated = valid_desktop_entry();

    let with_comments_and_blanks = format!(
        "# note\n\n{}",
        generated.replace("Name=My App\n", "# inline note\n\nName=My App\n")
    );
    assert!(
        super::desktop_entry_enabled(&with_comments_and_blanks),
        "comment lines and blank lines must stay enabled"
    );

    let with_localized_key = format!("{generated}Name[vi]=Ứng dụng\n");
    assert!(
        super::desktop_entry_enabled(&with_localized_key),
        "localized key must stay enabled"
    );

    let with_digits_and_dash_key = format!("{generated}X-Foo-2=1\n");
    assert!(
        super::desktop_entry_enabled(&with_digits_and_dash_key),
        "key with digits and '-' must stay enabled"
    );

    let with_second_group = format!(
        "{generated}\n[Desktop Action new]\nName=New Window\nExec=\"/opt/My App/bin/my-app\" --new\n"
    );
    assert!(
        super::desktop_entry_enabled(&with_second_group),
        "second group with valid lines after [Desktop Entry] must stay enabled"
    );
}

#[test]
fn linux_autostart_creates_reads_and_disables_entry() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("autostart");
    let identifier = "com.example.my-app";
    let product_name = "My App";
    let exe = Path::new("/opt/My App/bin/my-app");
    let file = dir.join(format!("{identifier}.desktop"));

    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, exe, None),
        Ok(AutostartStatus::Disabled),
        "missing folder and file must read as Disabled"
    );
    assert!(!dir.exists());

    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, exe, Some(true)),
        Ok(AutostartStatus::Enabled),
        "enabling must create folder and file and return Enabled"
    );
    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, exe, None),
        Ok(AutostartStatus::Enabled),
        "reading after enable must return Enabled"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        super::desktop_entry(product_name, exe)
    );

    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, exe, Some(false)),
        Ok(AutostartStatus::Disabled),
        "disabling must return Disabled"
    );
    assert!(!file.exists(), "disabling must remove the desktop file");
    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, exe, Some(false)),
        Ok(AutostartStatus::Disabled),
        "disabling when already missing must remain Ok(Disabled)"
    );
}

#[test]
fn linux_autostart_enable_twice_leaves_file_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("autostart");
    let identifier = "com.example.my-app";
    let product_name = "My App";
    let first_exe = Path::new("/opt/My App/v1/my-app");
    let second_exe = Path::new("/opt/My App/v2/my-app");
    let file = dir.join(format!("{identifier}.desktop"));

    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, first_exe, Some(true)),
        Ok(AutostartStatus::Enabled)
    );
    let initial_bytes = fs::read(&file).unwrap();

    assert_eq!(
        super::linux_autostart(&dir, identifier, product_name, second_exe, Some(true)),
        Ok(AutostartStatus::Enabled)
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        initial_bytes,
        "enabling a second time must leave the file byte for byte unchanged even when exe differs"
    );
}

#[test]
fn linux_autostart_leaves_hidden_and_gnome_disabled_files_alone() {
    let identifier = "com.example.my-app";
    let product_name = "My App";
    let exe = Path::new("/opt/My App/bin/my-app");
    let valid = super::desktop_entry(product_name, exe);

    let cases = [
        ("Hidden=true", format!("{valid}Hidden=true\n")),
        (
            "X-GNOME-Autostart-enabled=false",
            valid.replace(
                "X-GNOME-Autostart-enabled=true",
                "X-GNOME-Autostart-enabled=false",
            ),
        ),
    ];

    for (label, contents) in cases {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("autostart");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{identifier}.desktop"));
        fs::write(&file, &contents).unwrap();

        assert_eq!(
            super::linux_autostart(&dir, identifier, product_name, exe, None),
            Ok(AutostartStatus::Disabled),
            "{label}: reading must return Disabled"
        );
        assert_eq!(
            super::linux_autostart(&dir, identifier, product_name, exe, Some(false)),
            Ok(AutostartStatus::Disabled),
            "{label}: disabling must return Disabled"
        );
        assert!(
            file.exists(),
            "{label}: disabling must leave the file in place"
        );
        assert_eq!(
            fs::read(&file).unwrap(),
            contents.as_bytes(),
            "{label}: disabling must leave the file byte for byte unchanged"
        );
    }
}

#[test]
fn linux_autostart_replaces_malformed_files_on_enable() {
    let identifier = "com.example.my-app";
    let product_name = "My App";
    let exe = Path::new("/opt/My App/bin/my-app");
    let expected = super::desktop_entry(product_name, exe);
    let exec_line = "Exec=\"/opt/My App/bin/my-app\"";
    assert!(expected.contains(exec_line));

    let cases = [
        ("empty file", String::new()),
        (
            "missing [Desktop Entry] group",
            "Type=Application\nName=My App\nExec=\"/usr/bin/my-app\"\n".into(),
        ),
        (
            "missing Exec key",
            "[Desktop Entry]\nType=Application\nName=My App\n".into(),
        ),
        ("empty Exec value", expected.replace(exec_line, "Exec=")),
        (
            "unclosed quote in Exec",
            expected.replace(exec_line, "Exec=\"/opt/app"),
        ),
        ("space in key name", format!("{expected}Bad Key=1\n")),
        (
            "non key-value line",
            format!("{expected}not a key value line\n"),
        ),
    ];

    for (label, malformed) in cases {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("autostart");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{identifier}.desktop"));
        fs::write(&file, malformed).unwrap();

        assert_eq!(
            super::linux_autostart(&dir, identifier, product_name, exe, None),
            Ok(AutostartStatus::Disabled),
            "{label}: reading must return Disabled"
        );
        assert_eq!(
            super::linux_autostart(&dir, identifier, product_name, exe, Some(true)),
            Ok(AutostartStatus::Enabled),
            "{label}: enabling must return Enabled"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            expected,
            "{label}: enabling must replace malformed file with desktop_entry"
        );
    }
}

#[test]
fn linux_autostart_refuses_exe_path_with_equals() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("autostart");
    let identifier = "com.example.my-app";
    let exe = Path::new("/opt/a=b/app");
    let file = dir.join(format!("{identifier}.desktop"));

    assert!(!dir.exists());
    assert_eq!(
        super::linux_autostart(&dir, identifier, "My App", exe, Some(true)),
        Err(AutostartError::Failed),
        "executable path containing '=' must fail"
    );
    assert!(
        !dir.exists(),
        "refused executable path must not create the autostart folder"
    );
    assert!(
        !file.exists(),
        "refused executable path must not write a desktop file"
    );
}

#[test]
fn linux_autostart_refuses_bad_identifier() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("autostart");
    fs::create_dir_all(&dir).unwrap();
    let exe = Path::new("/opt/My App/bin/my-app");

    for identifier in ["../x", "", "singlelabel"] {
        for enabled in [None, Some(true), Some(false)] {
            assert_eq!(
                super::linux_autostart(&dir, identifier, "My App", exe, enabled),
                Err(AutostartError::Failed),
                "bad identifier {identifier:?} with enabled={enabled:?} must fail"
            );
        }
    }
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        0,
        "bad identifier must not write anything into autostart dir"
    );
    assert!(
        !temp.path().join("x.desktop").exists(),
        "path-traversal identifier must not write outside autostart dir"
    );
}

#[test]
fn windows_run_command_quotes_path_and_enforces_utf16_limit() {
    assert_eq!(
        super::windows_run_command(Path::new(r"C:\Program Files\My App\app.exe")),
        Some(r#""C:\Program Files\My App\app.exe""#.to_string())
    );
    assert_eq!(
        super::windows_run_command(Path::new(r"C:\Apps\yard.exe")),
        Some(r#""C:\Apps\yard.exe""#.to_string())
    );

    // 3 ("C:\") + 251 ('a') + 4 (".exe") + 2 quotes = 260 UTF-16 code units.
    let ascii_260 = format!(r"C:\{}.exe", "a".repeat(251));
    let expected_ascii_260 = format!("\"{ascii_260}\"");
    assert_eq!(expected_ascii_260.encode_utf16().count(), 260);
    assert_eq!(
        super::windows_run_command(Path::new(&ascii_260)),
        Some(expected_ascii_260)
    );

    // 3 ("C:\") + 252 ('a') + 4 (".exe") + 2 quotes = 261 UTF-16 code units.
    let ascii_261 = format!(r"C:\{}.exe", "a".repeat(252));
    assert_eq!(format!("\"{ascii_261}\"").encode_utf16().count(), 261);
    assert_eq!(super::windows_run_command(Path::new(&ascii_261)), None);

    // Non-BMP character '𝄞' (U+1D11E) takes 2 UTF-16 code units (1 char).
    // 3 ("C:\") + 2 ('𝄞') + 249 ('a') + 4 (".exe") + 2 quotes = 260 UTF-16 code units.
    let non_bmp_260 = format!(r"C:\𝄞{}.exe", "a".repeat(249));
    let expected_non_bmp_260 = format!("\"{non_bmp_260}\"");
    assert_eq!(expected_non_bmp_260.encode_utf16().count(), 260);
    assert_eq!(
        super::windows_run_command(Path::new(&non_bmp_260)),
        Some(expected_non_bmp_260)
    );

    // 3 ("C:\") + 2 ('𝄞') + 250 ('a') + 4 (".exe") + 2 quotes = 261 UTF-16 code units (260 chars).
    let non_bmp_261 = format!(r"C:\𝄞{}.exe", "a".repeat(250));
    assert_eq!(format!("\"{non_bmp_261}\"").encode_utf16().count(), 261);
    assert_eq!(super::windows_run_command(Path::new(&non_bmp_261)), None);
}

#[test]
fn windows_outcome_covers_read_and_enable_matrix() {
    let cases = [
        (None, false, false, (false, Ok(AutostartStatus::Disabled))),
        (None, false, true, (false, Ok(AutostartStatus::Disabled))),
        (None, true, false, (false, Ok(AutostartStatus::Enabled))),
        (
            None,
            true,
            true,
            (false, Ok(AutostartStatus::NeedsApproval)),
        ),
        (
            Some(true),
            false,
            false,
            (true, Ok(AutostartStatus::Enabled)),
        ),
        (
            Some(true),
            false,
            true,
            (true, Err(AutostartError::NeedsApproval)),
        ),
        (
            Some(true),
            true,
            false,
            (false, Ok(AutostartStatus::Enabled)),
        ),
        (
            Some(true),
            true,
            true,
            (false, Err(AutostartError::NeedsApproval)),
        ),
    ];

    for (enabled, run_present, approval_disabled, expected) in cases {
        assert_eq!(
            super::windows_outcome(enabled, run_present, approval_disabled),
            expected,
            "windows_outcome({enabled:?}, {run_present}, {approval_disabled})"
        );
    }
}

#[test]
fn startup_approved_disabled_checks_first_byte_parity_and_empty() {
    assert!(super::startup_approved_disabled(&[
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]));
    assert!(super::startup_approved_disabled(&[1, 0, 0, 0]));
    assert!(super::startup_approved_disabled(&[3]));

    assert!(!super::startup_approved_disabled(&[
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]));
    assert!(!super::startup_approved_disabled(&[0, 0, 0, 0]));
    assert!(!super::startup_approved_disabled(&[6, 0, 0, 0]));

    assert!(!super::startup_approved_disabled(&[]));
}

#[test]
fn in_app_bundle_detects_macos_bundle_executable() {
    assert!(super::in_app_bundle(Path::new(
        "/Applications/My App.app/Contents/MacOS/my-app"
    )));
    assert!(super::in_app_bundle(Path::new(
        "/Users/admin/Applications/Yard.app/Contents/MacOS/yard"
    )));

    assert!(!super::in_app_bundle(Path::new(
        "/Users/x/target/debug/yard"
    )));
    assert!(!super::in_app_bundle(Path::new("/usr/local/bin/yard")));
    assert!(!super::in_app_bundle(Path::new(
        "/Applications/My App.app/Contents/Resources/helper"
    )));
    assert!(!super::in_app_bundle(Path::new(
        "/Applications/NotABundle/Contents/MacOS/yard"
    )));
    assert!(!super::in_app_bundle(Path::new("/Contents/MacOS/yard")));

    // `in_app_bundle` itself expects a canonical path and rejects unnormalized
    // `..` components; the macOS backend canonicalizes the executable path first.
    assert!(!super::in_app_bundle(Path::new(
        "/Applications/A.app/Contents/MacOS/../MacOS/a"
    )));
}

#[test]
fn macos_status_maps_sm_app_service_values() {
    assert_eq!(super::macos_status(0), Some(AutostartStatus::Disabled));
    assert_eq!(super::macos_status(1), Some(AutostartStatus::Enabled));
    assert_eq!(super::macos_status(2), Some(AutostartStatus::NeedsApproval));
    assert_eq!(super::macos_status(3), Some(AutostartStatus::Disabled));

    for unknown in [-1, 4, isize::MAX] {
        assert_eq!(
            super::macos_status(unknown),
            None,
            "unknown SMAppService status {unknown} must map to None"
        );
    }

    for raw in (-100..=100).chain([isize::MIN, isize::MAX]) {
        assert_ne!(
            super::macos_status(raw),
            Some(AutostartStatus::NotSupported),
            "SMAppService status {raw} must never map to NotSupported"
        );
    }
}
