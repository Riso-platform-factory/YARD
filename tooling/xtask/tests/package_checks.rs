#![cfg(feature = "dev")]
//! `package_checks` runs every system's pre-build packaging checks on any
//! host, for every output that system can make, and `bundle macos`,
//! `bundle linux`, and `bundle windows` call it directly so their refusal
//! text matches.

#[allow(dead_code)]
mod support;

use std::path::Path;
use support::{FakeRunner, config_in, tempdir};
use xtask::{
    Arch, Interrupt, LinuxArgs, MacosArgs, PackageSystem, Secrets, WindowsArgs, WindowsHost,
    WindowsSecrets, bundle_linux, bundle_macos, bundle_windows, package_checks,
};

const SYSTEMS: [PackageSystem; 3] = [
    PackageSystem::Macos,
    PackageSystem::Linux,
    PackageSystem::Windows,
];
const ARCHES: [Arch; 2] = [Arch::X86_64, Arch::Aarch64];

fn host_arch() -> Arch {
    match std::env::consts::ARCH {
        "x86_64" => Arch::X86_64,
        "aarch64" => Arch::Aarch64,
        other => panic!("unsupported test host architecture: {other}"),
    }
}

fn windows_host(temp: &Path) -> WindowsHost {
    let inno = temp.join("inno");
    std::fs::create_dir_all(&inno).unwrap();
    std::fs::write(inno.join("ISCC.exe"), b"").unwrap();
    let root = temp.join("Windows");
    std::fs::create_dir_all(root.join("System32")).unwrap();
    for tool in ["tar.exe", "certutil.exe"] {
        std::fs::write(root.join("System32").join(tool), b"").unwrap();
    }
    WindowsHost {
        system: "windows".into(),
        path: Some(inno.into_os_string()),
        program_files_x86: None,
        system_root: Some(root.into_os_string()),
        xtask: None,
    }
}

#[test]
fn sample_config_passes_for_every_system_and_arch() {
    let file = xtask::repository().join("app/yard/product.toml");
    let product = yard_build_config::read(&file).unwrap();
    for system in SYSTEMS {
        for arch in ARCHES {
            assert_eq!(
                package_checks(&file, &product, system, arch),
                Ok(()),
                "sample config should pass for {system:?} {arch:?}"
            );
        }
    }
}

#[test]
fn name_with_slash_fails_on_all_three_systems() {
    let temp = tempdir();
    let file = config_in(
        temp.path(),
        "Lab/Notes",
        "0.1.0",
        "test.yard.slash",
        "yardslash",
    );
    let product = yard_build_config::read(&file).unwrap();
    let path_text = file.display().to_string();
    for system in SYSTEMS {
        for arch in ARCHES {
            let Err(reason) = package_checks(&file, &product, system, arch) else {
                panic!("Lab/Notes must fail on {system:?} {arch:?}");
            };
            assert!(
                reason.contains(&path_text),
                "{system:?} {arch:?}: reason should name the config file: {reason}"
            );
        }
    }
}

#[test]
fn version_with_slash_fails_on_all_three_systems() {
    let temp = tempdir();
    let file = config_in(
        temp.path(),
        "Lab",
        "0.1/0",
        "test.yard.slashver",
        "yardslashver",
    );
    let product = yard_build_config::read(&file).unwrap();
    let path_text = file.display().to_string();
    for system in SYSTEMS {
        for arch in ARCHES {
            let Err(reason) = package_checks(&file, &product, system, arch) else {
                panic!("version 0.1/0 must fail on {system:?} {arch:?}");
            };
            assert!(
                reason.contains(&path_text),
                "{system:?} {arch:?}: reason should name the config file: {reason}"
            );
        }
    }
}

#[test]
fn windows_device_name_fails_on_windows_only() {
    let temp = tempdir();
    let file = config_in(temp.path(), "CON", "0.1.0", "test.yard.con", "yardcon");
    let product = yard_build_config::read(&file).unwrap();
    let path_text = file.display().to_string();
    for arch in ARCHES {
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Macos, arch),
            Ok(()),
            "CON should pass on macOS {arch:?}"
        );
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Linux, arch),
            Ok(()),
            "CON should pass on Linux {arch:?}"
        );
        let Err(reason) = package_checks(&file, &product, PackageSystem::Windows, arch) else {
            panic!("CON must fail on Windows {arch:?}");
        };
        assert!(
            reason.contains(&path_text),
            "Windows {arch:?}: reason should name the config file: {reason}"
        );
    }
}

#[test]
fn identifier_of_128_characters_fails_on_windows_only() {
    let temp = tempdir();
    let identifier = format!("test.{}", "a".repeat(123));
    assert_eq!(identifier.len(), 128);
    let file = config_in(temp.path(), "Lab", "0.1.0", &identifier, "yardlab");
    let product = yard_build_config::read(&file).unwrap();
    let path_text = file.display().to_string();
    for arch in ARCHES {
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Macos, arch),
            Ok(()),
            "128-char identifier should pass on macOS {arch:?}"
        );
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Linux, arch),
            Ok(()),
            "128-char identifier should pass on Linux {arch:?}"
        );
        let Err(reason) = package_checks(&file, &product, PackageSystem::Windows, arch) else {
            panic!("128-char identifier must fail on Windows {arch:?}");
        };
        assert!(
            reason.contains(&path_text),
            "Windows {arch:?}: reason should name the config file: {reason}"
        );
    }
}

#[test]
fn arch_matters_for_windows_file_name_limit() {
    let temp = tempdir();
    // `<Name>-0.1.0-windows-x86_64.zip` is 255 UTF-16 units;
    // `<Name>-0.1.0-windows-aarch64.zip` is 256 UTF-16 units.
    let name = "a".repeat(255 - "-0.1.0-windows-x86_64.zip".len());
    let file = config_in(temp.path(), &name, "0.1.0", "test.yard.long", "yardlong");
    let product = yard_build_config::read(&file).unwrap();

    assert_eq!(
        package_checks(&file, &product, PackageSystem::Windows, Arch::X86_64),
        Ok(()),
        "255-character Windows zip name fits on x86_64"
    );
    let reason = package_checks(&file, &product, PackageSystem::Windows, Arch::Aarch64)
        .expect_err("256-character Windows zip name must fail on aarch64");
    assert!(
        reason.contains(&file.display().to_string()),
        "reason should name the config file: {reason}"
    );
    assert!(
        reason.contains("255"),
        "reason should name Windows' 255-character limit: {reason}"
    );
}

#[test]
fn arch_matters_for_linux_archive_name_limit() {
    let temp = tempdir();
    // `<name>-0.1.0-linux-x86_64.tar.gz` is 255 bytes;
    // `<name>-0.1.0-linux-aarch64.tar.gz` is 256 bytes.
    let name = "a".repeat(255 - "-0.1.0-linux-x86_64.tar.gz".len());
    let file = config_in(
        temp.path(),
        &name,
        "0.1.0",
        "test.yard.linuxlong",
        "yardlinuxlong",
    );
    let product = yard_build_config::read(&file).unwrap();

    for arch in ARCHES {
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Macos, arch),
            Ok(()),
            "229-byte name fits on macOS {arch:?}"
        );
        assert_eq!(
            package_checks(&file, &product, PackageSystem::Windows, arch),
            Ok(()),
            "229-byte name fits on Windows {arch:?}"
        );
    }
    assert_eq!(
        package_checks(&file, &product, PackageSystem::Linux, Arch::X86_64),
        Ok(()),
        "255-byte Linux archive name fits on x86_64"
    );
    let reason = package_checks(&file, &product, PackageSystem::Linux, Arch::Aarch64)
        .expect_err("256-byte Linux archive name must fail on aarch64");
    assert!(
        reason.contains(&file.display().to_string()),
        "reason should name the config file: {reason}"
    );
    assert!(
        reason.contains("255"),
        "reason should name the 255-byte limit: {reason}"
    );
}

#[test]
fn linux_refuses_identifier_whose_desktop_file_exceeds_255_bytes() {
    let max_temp = tempdir();
    let max_identifier = format!(
        "test.{}",
        "a".repeat(255 - ".desktop".len() - "test.".len())
    );
    assert_eq!(max_identifier.len() + ".desktop".len(), 255);
    let max_file = config_in(
        max_temp.path(),
        "Lab",
        "0.1.0",
        &max_identifier,
        "yarddesktopok",
    );
    let max_product = yard_build_config::read(&max_file).unwrap();

    let over_temp = tempdir();
    let over_identifier = format!(
        "test.{}",
        "a".repeat(256 - ".desktop".len() - "test.".len())
    );
    assert_eq!(over_identifier.len() + ".desktop".len(), 256);
    let over_file = config_in(
        over_temp.path(),
        "Lab",
        "0.1.0",
        &over_identifier,
        "yarddesktopover",
    );
    let over_product = yard_build_config::read(&over_file).unwrap();
    let over_path_text = over_file.display().to_string();

    for arch in ARCHES {
        assert_eq!(
            package_checks(&max_file, &max_product, PackageSystem::Linux, arch),
            Ok(()),
            "255-byte <identifier>.desktop should pass on Linux {arch:?}"
        );
        let reason = package_checks(&over_file, &over_product, PackageSystem::Linux, arch)
            .expect_err("256-byte <identifier>.desktop must fail on Linux");
        assert!(
            reason.contains(&over_path_text),
            "Linux {arch:?}: reason should name the config file: {reason}"
        );
        assert!(
            reason.contains("255"),
            "Linux {arch:?}: reason should name the 255-byte limit: {reason}"
        );
        assert_eq!(
            package_checks(&over_file, &over_product, PackageSystem::Macos, arch),
            Ok(()),
            "macOS does not create <identifier>.desktop ({arch:?})"
        );
    }
}

#[test]
fn bundle_refusals_match_package_checks_reason_text() {
    let arch = host_arch();

    // macOS
    {
        let temp = tempdir();
        let file = config_in(
            temp.path(),
            "Lab/Notes",
            "0.1.0",
            "test.yard.slash",
            "yardslash",
        );
        let product = yard_build_config::read(&file).unwrap();
        let expected = package_checks(&file, &product, PackageSystem::Macos, arch).unwrap_err();
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &MacosArgs {
                config: Some(file.clone()),
                out: Some(temp.path().join("out")),
                ..MacosArgs::default()
            },
            &Secrets::default(),
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "config");
        assert_eq!(failure.reason, expected);
        support::assert_only_metadata(&runner);
    }

    // Linux
    {
        let temp = tempdir();
        let file = config_in(temp.path(), "Lab", "0.1/0", "test.yard.linux", "yardlinux");
        let product = yard_build_config::read(&file).unwrap();
        let expected = package_checks(&file, &product, PackageSystem::Linux, arch).unwrap_err();
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_linux(
            &LinuxArgs {
                config: Some(file.clone()),
                out: Some(temp.path().join("out")),
                ..LinuxArgs::default()
            },
            "linux",
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "config");
        assert_eq!(failure.reason, expected);
        support::assert_only_metadata(&runner);
    }

    // Windows
    {
        let temp = tempdir();
        let file = config_in(temp.path(), "CON", "0.1.0", "test.yard.win", "yardwin");
        let product = yard_build_config::read(&file).unwrap();
        let expected = package_checks(&file, &product, PackageSystem::Windows, arch).unwrap_err();
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_windows(
            &WindowsArgs {
                config: Some(file.clone()),
                out: Some(temp.path().join("out")),
                ..WindowsArgs::default()
            },
            &windows_host(temp.path()),
            &WindowsSecrets::default(),
            &Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "config");
        assert_eq!(failure.reason, expected);
        support::assert_only_metadata(&runner);
    }
}
