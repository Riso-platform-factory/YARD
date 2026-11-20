#![cfg(feature = "dev")]
//! `bundle macos` lays out `<name>.app` from each config under test, with
//! the commands it would run recorded instead of run: the bundle's paths and
//! `Info.plist` equal the config, a review bundle runs `dev`, and what breaks
//! a rule is refused before anything runs.

#[allow(dead_code)]
mod support;

use std::path::Path;
use support::{FakeRunner, Plist, config_in, parse_plist, tempdir, under_test};
use xtask::{MacosArgs, bundle_macos};

fn options(config: &Path, out: &Path) -> MacosArgs {
    MacosArgs {
        config: Some(config.to_owned()),
        out: Some(out.to_owned()),
        ..MacosArgs::default()
    }
}

fn plist_of(app: &Path) -> Plist {
    parse_plist(&std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap())
}

fn entries(out: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn each_config_lays_out_its_bundle() {
    for (folder, name, version, identifier, scheme) in [
        ("plain", "Lab", "0.1.0", "test.yard.lab", "yardlab"),
        (
            "spaced",
            "Lab Notes Pro",
            "0.2.0",
            "test.yard.labnotes",
            "yardlabnotes",
        ),
        (
            "other",
            "Other Lab",
            "1.0.0",
            "test.yard.other",
            "yardother",
        ),
    ] {
        let temp = tempdir();
        let out = temp.path().join("out");
        let config = under_test(folder);
        let mut runner = FakeRunner::new(temp.path());
        let bundled = bundle_macos(
            &options(&config, &out),
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap();

        let app = out.join(format!("{name}.app"));
        assert_eq!(bundled.app, app);
        assert_eq!(bundled.zip, None);
        assert_eq!(bundled.dmg, None);
        assert_eq!(
            entries(&out),
            [format!("{name}.app")],
            "{folder}: only the bundle stays"
        );
        assert_eq!(
            std::fs::read(app.join("Contents/MacOS").join(name)).unwrap(),
            b"the yard binary",
            "{folder}: the executable is the reported binary"
        );
        assert_eq!(entries(&app.join("Contents/MacOS")), [name]);
        assert_eq!(
            entries(&app.join("Contents/Resources")),
            [format!("{name}.icns")]
        );

        let plist = plist_of(&app);
        let expected = Plist::Dict(vec![
            ("CFBundleName".into(), Plist::string(name)),
            ("CFBundleDisplayName".into(), Plist::string(name)),
            ("CFBundleIdentifier".into(), Plist::string(identifier)),
            ("CFBundleShortVersionString".into(), Plist::string(version)),
            ("CFBundleVersion".into(), Plist::string(version)),
            ("CFBundleExecutable".into(), Plist::string(name)),
            (
                "CFBundleIconFile".into(),
                Plist::string(&format!("{name}.icns")),
            ),
            ("CFBundlePackageType".into(), Plist::string("APPL")),
            (
                "CFBundleURLTypes".into(),
                Plist::Array(vec![Plist::Dict(vec![
                    ("CFBundleURLName".into(), Plist::string(identifier)),
                    (
                        "CFBundleURLSchemes".into(),
                        Plist::Array(vec![Plist::string(scheme)]),
                    ),
                ])]),
            ),
            ("LSMinimumSystemVersion".into(), Plist::string("12.0")),
            ("NSHighResolutionCapable".into(), Plist::Bool(true)),
        ]);
        assert_eq!(plist, expected, "{folder}: Info.plist");

        let programs = runner.programs();
        assert_eq!(
            programs,
            [
                ["cargo"; 2].as_slice(),
                &["sips"; 10],
                &["iconutil", "plutil", "codesign", "codesign"]
            ]
            .concat(),
            "{folder}: the commands in order"
        );
        let stages = &runner.commands[0];
        assert_eq!(stages.args, ["test", "-p", "yard", "--test", "stages"]);
        // `get_envs` lists the variables sorted by name.
        let secrets_removed = || {
            [
                "APPLE_NOTARIZATION_ISSUER_ID",
                "APPLE_NOTARIZATION_KEY",
                "APPLE_NOTARIZATION_KEY_ID",
                "MACOS_CERTIFICATE",
                "MACOS_CERTIFICATE_PASSWORD",
                "WINDOWS_CERTIFICATE",
                "WINDOWS_CERTIFICATE_PASSWORD",
            ]
            .map(|name| (name.to_owned(), None))
        };
        assert_eq!(
            stages.envs,
            [
                secrets_removed().as_slice(),
                &[("YARD_PROMOTE".to_owned(), None)]
            ]
            .concat(),
            "{folder}: no secret reaches cargo"
        );
        assert_eq!(stages.dir.as_deref(), Some(xtask::repository().as_path()));
        let build = &runner.commands[1];
        assert_eq!(
            build.args,
            [
                "build",
                "--release",
                "-p",
                "yard",
                "--bin",
                "yard",
                "--no-default-features",
                "--message-format=json-render-diagnostics"
            ]
        );
        let mut expected = [
            secrets_removed().as_slice(),
            &[(
                "PRODUCT_CONFIG".to_owned(),
                Some(config.display().to_string()),
            )],
        ]
        .concat();
        expected.sort();
        assert_eq!(build.envs, expected, "{folder}: no secret reaches cargo");
        assert_eq!(build.dir.as_deref(), Some(xtask::repository().as_path()));
        let icon = config.with_file_name("icon.png").display().to_string();
        let sizes: Vec<(String, String)> = runner.commands[2..12]
            .iter()
            .map(|sips| {
                assert_eq!(sips.args[0], "-z");
                assert_eq!(sips.args[1], sips.args[2]);
                assert_eq!(sips.args[3], icon);
                assert_eq!(sips.args[4], "--out");
                let file = Path::new(&sips.args[5]).file_name().unwrap();
                (sips.args[1].clone(), file.to_string_lossy().into_owned())
            })
            .collect();
        let expected_sizes: Vec<(String, String)> = [16, 32, 128, 256, 512]
            .into_iter()
            .flat_map(|size| {
                [
                    (size.to_string(), format!("icon_{size}x{size}.png")),
                    ((size * 2).to_string(), format!("icon_{size}x{size}@2x.png")),
                ]
            })
            .collect();
        assert_eq!(sizes, expected_sizes, "{folder}: the icon sizes");
        assert_eq!(runner.commands[13].args[0], "-lint");
        assert_eq!(
            runner.commands[14].args[..4],
            ["--force", "--deep", "--sign", "-"]
        );
        assert_eq!(
            runner.commands[15].args[..3],
            ["--verify", "--deep", "--strict"]
        );
    }
}

#[test]
fn a_dev_bundle_wraps_the_binary_to_run_dev_with_its_environment() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let mut runner = FakeRunner::new(temp.path());
    let options = MacosArgs {
        dev: true,
        env: vec!["YARD_HOME=/tmp/a b".into(), "_X1=1 & <2>".into()],
        ..options(&under_test("plain"), &out)
    };
    let bundled = bundle_macos(
        &options,
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();

    let macos = bundled.app.join("Contents/MacOS");
    assert_eq!(entries(&macos), ["Lab", "Lab-bin"]);
    assert_eq!(
        std::fs::read_to_string(macos.join("Lab")).unwrap(),
        xtask::WRAPPER
    );
    assert_eq!(xtask::WRAPPER, "#!/bin/sh\nexec \"$0-bin\" dev \"$@\"\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(macos.join("Lab"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the wrapper is executable");
    }
    assert_eq!(
        std::fs::read(macos.join("Lab-bin")).unwrap(),
        b"the yard binary"
    );

    let plist = plist_of(&bundled.app);
    assert_eq!(plist.keys().last(), Some(&"LSEnvironment"));
    assert_eq!(
        plist.get("LSEnvironment"),
        Some(&Plist::Dict(vec![
            ("YARD_HOME".into(), Plist::string("/tmp/a b")),
            ("_X1".into(), Plist::string("1 & <2>")),
        ]))
    );
    let build = &runner.commands[1];
    assert!(
        !build.args.iter().any(|arg| arg == "--no-default-features"),
        "a review bundle builds with dev: {:?}",
        build.args
    );
}

#[test]
fn a_dev_bundle_must_differ_from_the_production_product() {
    for (name, identifier, scheme, key) in [
        ("YARD", "test.yard.devname", "yarddevname", "name"),
        ("Yard Dev", "VN.RisoTech.Yard", "yarddevid", "identifier"),
        ("Yard Dev", "test.yard.devscheme", "yard", "url_scheme"),
    ] {
        let temp = tempdir();
        let config = config_in(temp.path(), name, "0.1.0", identifier, scheme);
        let mut runner = FakeRunner::new(temp.path());
        let options = MacosArgs {
            dev: true,
            ..options(&config, &temp.path().join("out"))
        };
        let failure = bundle_macos(
            &options,
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "config");
        assert_eq!(
            failure.reason,
            format!(
                "{}: --dev requires a `{key}` different from the production product's",
                config.display()
            )
        );
        assert!(runner.commands.is_empty(), "nothing ran");
        assert!(!temp.path().join("out").exists());
    }
}

#[test]
fn env_needs_dev_and_a_shell_style_key() {
    let temp = tempdir();
    let config = under_test("plain");
    let out = temp.path().join("out");
    for (dev, env, reason) in [
        (false, "YARD_HOME=/tmp", "--env requires --dev".to_owned()),
        (
            true,
            "1KEY=value",
            "--env \"1KEY=value\" requires KEY=VALUE with a shell-style KEY".to_owned(),
        ),
        (
            true,
            "NOVALUE",
            "--env \"NOVALUE\" requires KEY=VALUE with a shell-style KEY".to_owned(),
        ),
    ] {
        let mut runner = FakeRunner::new(temp.path());
        let options = MacosArgs {
            dev,
            env: vec![env.into()],
            ..options(&config, &out)
        };
        let failure = bundle_macos(
            &options,
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!((failure.step, failure.reason), ("arguments", reason));
        assert!(runner.commands.is_empty());
    }
}

#[test]
fn a_bad_config_is_refused_with_the_config_readers_message() {
    let configs = xtask::repository().join("crates/yard_build_config/tests/configs");
    let mut bad = 0;
    for entry in std::fs::read_dir(configs).unwrap() {
        let folder = entry.unwrap().path();
        if !folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("bad-")
        {
            continue;
        }
        bad += 1;
        let config = folder.join("product.toml").canonicalize().unwrap();
        let temp = tempdir();
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &options(&config, &temp.path().join("out")),
            &xtask::Secrets::default(),
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "config");
        assert_eq!(
            failure.reason,
            yard_build_config::read(&config).unwrap_err()
        );
        assert!(runner.commands.is_empty());
    }
    assert!(bad >= 1, "the bad configs are found");
}

#[test]
fn names_are_single_path_components() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let slashed_name = config_in(
        temp.path(),
        "Lab/Notes",
        "0.1.0",
        "test.yard.slash",
        "yardslash",
    );
    let failure = bundle_macos(
        &options(&slashed_name, &out),
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut FakeRunner::new(temp.path()),
    )
    .unwrap_err();
    assert_eq!(failure.step, "config");
    assert_eq!(
        failure.reason,
        format!(
            "{}: name must be a single path component",
            slashed_name.display()
        )
    );

    let temp = tempdir();
    let out = temp.path().join("out");
    let slashed_version = config_in(temp.path(), "Lab", "0.1/0", "test.yard.slash", "yardslash");
    let zipped = MacosArgs {
        zip: true,
        ..options(&slashed_version, &out)
    };
    let mut runner = FakeRunner::new(temp.path());
    let failure = bundle_macos(
        &zipped,
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "config");
    assert_eq!(
        failure.reason,
        format!(
            "{}: version must be a single path component with --zip",
            slashed_version.display()
        )
    );
    assert!(runner.commands.is_empty());

    // Without --zip, the version names no file.
    bundle_macos(
        &options(&slashed_version, &out),
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut FakeRunner::new(temp.path()),
    )
    .unwrap();
}

#[test]
fn a_zip_is_staged_hashed_and_published() {
    let temp = tempdir();
    let out = temp.path().join("out");
    std::fs::create_dir_all(out.join("Lab Notes Pro.app")).unwrap();
    std::fs::write(out.join("Lab Notes Pro.app/old"), b"old").unwrap();
    let mut runner = FakeRunner::new(temp.path());
    let options = MacosArgs {
        zip: true,
        ..options(&under_test("spaced"), &out)
    };
    let bundled = bundle_macos(
        &options,
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    let archive = out.join("Lab Notes Pro-0.2.0.zip");
    assert_eq!(
        bundled.zip,
        Some((archive.clone(), support::DIGEST.to_owned()))
    );
    assert_eq!(
        entries(&out),
        ["Lab Notes Pro-0.2.0.zip", "Lab Notes Pro.app"]
    );
    assert!(
        !out.join("Lab Notes Pro.app/old").exists(),
        "the old bundle is replaced"
    );
    let ditto = runner
        .commands
        .iter()
        .find(|command| command.program == "ditto")
        .unwrap();
    assert_eq!(ditto.args[..3], ["-c", "-k", "--keepParent"]);
    let staged = Path::new(&ditto.args[4]);
    assert_eq!(
        ditto.args[3],
        staged
            .with_file_name("Lab Notes Pro.app")
            .display()
            .to_string(),
        "the zip is made from the staged app, before anything is published"
    );
    assert_eq!(
        staged.parent().unwrap().parent(),
        Some(out.as_path()),
        "staged inside out"
    );
    let shasum = runner.commands.last().unwrap();
    assert_eq!(shasum.program, "shasum");
    assert_eq!(shasum.args, ["-a", "256", ditto.args[4].as_str()]);
}

#[test]
fn a_failed_replacement_puts_the_previous_bundle_back() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let previous = out.join("Lab.app");
    std::fs::create_dir_all(&previous).unwrap();
    std::fs::write(previous.join("old"), b"old").unwrap();
    let mut runner = FakeRunner::new(temp.path());
    // The staged bundle disappears after it is signed, so moving it into
    // place fails after the previous one was moved aside.
    runner.on_verify = Some(Box::new(|bundle: &Path| {
        std::fs::remove_dir_all(bundle).unwrap()
    }));
    let failure = bundle_macos(
        &options(&under_test("plain"), &out),
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "bundle");
    assert!(
        failure.reason.starts_with("could not move "),
        "{}",
        failure.reason
    );
    assert_eq!(std::fs::read(previous.join("old")).unwrap(), b"old");
    assert_eq!(entries(&out), ["Lab.app"], "the temporary folder is gone");
}

#[test]
fn a_failed_step_leaves_the_previous_bundle_alone() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let previous = out.join("Lab.app");
    std::fs::create_dir_all(&previous).unwrap();
    std::fs::write(previous.join("old"), b"old").unwrap();
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("codesign");
    let failure = bundle_macos(
        &options(&under_test("plain"), &out),
        &xtask::Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "sign");
    assert_eq!(std::fs::read(previous.join("old")).unwrap(), b"old");
    assert_eq!(entries(&out), ["Lab.app"]);
}
