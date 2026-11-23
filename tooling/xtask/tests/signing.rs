#![cfg(all(feature = "dev", target_os = "macos"))]
//! Signing with the developer's identity, notarization, and the disk image,
//! with the commands recorded instead of run: their order and flags, the
//! secrets kept out of every argument, the refusals before building, and the
//! temporary keychain undone after a failure.

#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use support::{
    DIGEST, FakeRunner, LOGIN_KEYCHAIN, Recorded, base64, config_in, signed_config, tempdir,
};
use xtask::{MacosArgs, Secrets, bundle_macos};

const IDENTITY: &str = "Developer ID Application: YARD Test (YARDTEST01)";
const CERTIFICATE: &[u8] = b"p12 bytes of the identity hunter2";
const PASSWORD: &str = "pass \"word\" hunter2";
const KEY: &str =
    "-----BEGIN PRIVATE KEY-----\nnotarization key hunter2\n-----END PRIVATE KEY-----\n";

fn secrets() -> Secrets {
    Secrets {
        certificate: Some(base64(CERTIFICATE)),
        certificate_password: Some(PASSWORD.into()),
        notarization_key: Some(base64(KEY.as_bytes())),
        notarization_key_id: Some("KEYID12345".into()),
        notarization_issuer_id: Some("issuer-uuid".into()),
    }
}

fn options(config: &Path, out: &Path) -> MacosArgs {
    MacosArgs {
        config: Some(config.to_owned()),
        out: Some(out.to_owned()),
        ..MacosArgs::default()
    }
}

/// Each command as its program and arguments, in order.
fn lines(runner: &FakeRunner) -> Vec<Vec<String>> {
    runner
        .commands
        .iter()
        .map(|command| {
            std::iter::once(command.program.clone())
                .chain(command.args.iter().cloned())
                .collect()
        })
        .collect()
}

fn find<'a>(runner: &'a FakeRunner, program: &str, first: &str) -> Vec<&'a Recorded> {
    runner
        .commands
        .iter()
        .filter(|command| command.program == program && command.args[0] == first)
        .collect()
}

/// Every argument and environment value of every command, and the failure,
/// holds none of the secrets.
fn no_secret_in(runner: &FakeRunner, also: &str) {
    let keychain_password = runner
        .commands
        .iter()
        .filter_map(|command| command.input.as_deref())
        .find_map(|input| input.strip_prefix("create-keychain -p \""))
        .map(|rest| rest.split('"').next().unwrap().to_owned());
    let mut secrets = vec![
        base64(CERTIFICATE),
        String::from_utf8(CERTIFICATE.to_vec()).unwrap(),
        PASSWORD.to_owned(),
        "hunter2".to_owned(),
        base64(KEY.as_bytes()),
        KEY.to_owned(),
    ];
    secrets.extend(keychain_password);
    for command in &runner.commands {
        let words = command
            .args
            .iter()
            .chain(command.envs.iter().filter_map(|(_, value)| value.as_ref()));
        for word in words.chain([&also.to_owned()]) {
            for secret in &secrets {
                assert!(!word.contains(secret.as_str()), "a secret in {word:?}");
            }
        }
    }
}

#[test]
fn an_ad_hoc_app_gets_an_unsigned_disk_image() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = config_in(temp.path(), "Lab", "0.1.0", "test.yard.dmg", "yarddmg");
    let text = std::fs::read_to_string(&config).unwrap() + "[release.macos]\ndmg = true\n";
    std::fs::write(&config, text).unwrap();
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();

    let image = out.join("Lab-0.1.0.dmg");
    assert_eq!(bundled.dmg, Some((image.clone(), DIGEST.to_owned())));
    assert!(image.is_file());
    let programs = runner.programs();
    assert_eq!(
        programs[13..],
        [
            "iconutil", "plutil", "codesign", "codesign", "ditto", "hdiutil", "shasum"
        ],
        "ad hoc as without a disk image, then the image, signed by nothing"
    );
    assert_eq!(programs[..3], ["cargo", "cargo", "cargo"]);
    let codesigns = find(&runner, "codesign", "--force");
    assert_eq!(codesigns.len(), 1);
    assert_eq!(codesigns[0].args[..4], ["--force", "--deep", "--sign", "-"]);
    let hdiutil = find(&runner, "hdiutil", "create");
    let args = &hdiutil[0].args;
    assert_eq!(args[..4], ["create", "-volname", "Lab", "-srcfolder"]);
    assert_eq!(args[5..8], ["-ov", "-format", "UDZO"]);
    let staging = PathBuf::from(&args[4]);
    let copy = find(
        &runner,
        "ditto",
        &staging.with_file_name("Lab.app").display().to_string(),
    );
    assert_eq!(
        copy[0].args[1],
        staging.join("Lab.app").display().to_string(),
        "the staged app is copied into the image's folder"
    );
    assert!(!staging.exists(), "the staging folder is removed");
    assert!(
        !lines(&runner)
            .iter()
            .any(|line| line[0] == "security" || line[0] == "xcrun")
    );
}

#[test]
fn an_identity_from_the_existing_keychains_signs_the_binary_then_the_app() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    let bundled = bundle_macos(
        &options(&config, &out),
        &Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(bundled.dmg, None);

    let entitlements = temp
        .path()
        .join("entitlements.plist")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    let signed = |path: &Path| -> Vec<String> {
        [
            "codesign",
            "--force",
            "--options",
            "runtime",
            "--timestamp",
            "--entitlements",
            &entitlements,
            "--sign",
            IDENTITY,
            &path.display().to_string(),
        ]
        .map(str::to_owned)
        .to_vec()
    };
    let lines = lines(&runner);
    let staged = PathBuf::from(&lines[14][2])
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    assert_eq!(
        lines[15..],
        [
            signed(&staged.join("Contents/MacOS/Signed Lab")),
            signed(&staged),
            vec![
                "codesign".into(),
                "--verify".into(),
                "--deep".into(),
                "--strict".into(),
                staged.display().to_string()
            ],
        ],
        "no keychain is made and nothing is notarized"
    );
}

#[test]
fn a_certificate_notarization_and_a_disk_image_run_in_zeds_order() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let mut runner = FakeRunner::new(temp.path());
    let options = MacosArgs {
        zip: true,
        ..options(&config, &out)
    };
    let bundled = bundle_macos(
        &options,
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    let app = out.join("Signed Lab.app");
    let image = out.join("Signed Lab-0.3.0.dmg");
    assert_eq!(bundled.dmg, Some((image.clone(), DIGEST.to_owned())));
    assert_eq!(
        bundled.zip,
        Some((out.join("Signed Lab-0.3.0.zip"), DIGEST.to_owned()))
    );

    let lines: Vec<Vec<String>> = lines(&runner)
        .into_iter()
        .filter(|line| line[0] != "sips")
        .collect();
    let summary: Vec<String> = lines
        .iter()
        .map(|line| match line[0].as_str() {
            "security" | "xcrun" => format!("{} {}", line[0], line[1]),
            "codesign" if line[1] == "--verify" => "codesign --verify".into(),
            program => program.to_owned(),
        })
        .collect();
    assert_eq!(
        summary,
        [
            "cargo",
            "cargo",
            "cargo",
            "security -i",
            "security list-keychains",
            "security list-keychains",
            "security set-keychain-settings",
            "security -i",
            "security -i",
            "security -i",
            "iconutil",
            "plutil",
            "codesign",
            "codesign",
            "codesign --verify",
            "ditto",
            "xcrun notarytool",
            "xcrun stapler",
            "ditto",
            "shasum",
            "ditto",
            "hdiutil",
            "codesign",
            "xcrun notarytool",
            "xcrun stapler",
            "shasum",
            "security delete-keychain",
        ]
    );

    // The temporary keychain, in a private folder, first in the search list.
    let inputs: Vec<&str> = runner
        .commands
        .iter()
        .filter_map(|command| command.input.as_deref())
        .collect();
    assert_eq!(inputs.len(), 4);
    let keychain = inputs[0]
        .split('"')
        .nth(3)
        .map(PathBuf::from)
        .expect("create-keychain names the keychain");
    let private = keychain.parent().unwrap().to_owned();
    assert!(inputs[0].starts_with("create-keychain -p \""));
    assert!(inputs[1].starts_with("unlock-keychain -p \""));
    assert!(inputs[2].starts_with("import \""));
    assert!(
        inputs[2].contains(" -f pkcs12 -P \"pass \\\"word\\\" hunter2\" -T /usr/bin/codesign\n"),
        "the password goes through stdin, quoted: {}",
        inputs[2]
    );
    assert!(
        inputs[3].starts_with("set-key-partition-list -S apple-tool:,apple:,codesign: -s -k \"")
    );
    assert_eq!(
        runner.certificate_seen,
        Some((CERTIFICATE.to_vec(), 0o600)),
        "the certificate is written only for the user"
    );
    let keychain = keychain.display().to_string();
    // The search list is read and set right after the keychain is made.
    assert_eq!(lines[4], ["security", "list-keychains", "-d", "user"]);
    assert_eq!(
        lines[5],
        [
            "security",
            "list-keychains",
            "-d",
            "user",
            "-s",
            &keychain,
            LOGIN_KEYCHAIN
        ]
    );

    // Notarized with the key in a private file, then stapled.
    let submits = find(&runner, "xcrun", "notarytool");
    assert_eq!(submits.len(), 2);
    let submit = &submits[0].args;
    assert_eq!(submit[..2], ["notarytool", "submit"]);
    assert!(submit[2].ends_with("notarize.zip"));
    assert_eq!(submit[3], "--key");
    assert!(Path::new(&submit[4]).starts_with(&private));
    assert_eq!(
        submit[5..],
        [
            "--key-id",
            "KEYID12345",
            "--issuer",
            "issuer-uuid",
            "--wait",
            "--output-format",
            "json"
        ]
    );
    assert_eq!(runner.key_seen.as_deref(), Some(KEY.as_bytes()));
    let staples = find(&runner, "xcrun", "stapler");
    let staged_app = &lines[15][4];
    assert_eq!(staples[0].args, ["stapler", "staple", staged_app.as_str()]);
    assert_eq!(lines[15][..4], ["ditto", "-c", "-k", "--keepParent"]);
    // The zip is made after stapling, from the staged app.
    assert_eq!(&lines[18][4], staged_app);
    assert!(app.is_dir());

    // The disk image is signed without entitlements, under an identifier of
    // its own, notarized, stapled.
    let image_signature = &lines[22];
    assert_eq!(
        image_signature[1..10],
        [
            "--force",
            "--options",
            "runtime",
            "--timestamp",
            "--identifier",
            "test.yard.signed.dmg",
            "--sign",
            IDENTITY,
            &lines[21][9]
        ]
    );
    assert_eq!(submits[1].args[2], lines[21][9]);
    assert_eq!(staples[1].args[2], lines[21][9]);
    assert!(image.is_file());

    // Undone: the keychain deleted, which takes it out of the search list,
    // and the folder gone.
    assert_eq!(
        lines[lines.len() - 1],
        ["security", "delete-keychain", &keychain]
    );
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
    assert!(!private.exists(), "the private folder is removed");
    no_secret_in(&runner, "");
}

#[test]
fn notarizing_without_the_api_key_is_refused_before_building() {
    let temp = tempdir();
    let config = signed_config(temp.path(), true, false);
    for (secrets, missing) in [
        (
            Secrets {
                notarization_key: None,
                notarization_key_id: None,
                notarization_issuer_id: None,
                ..secrets()
            },
            "APPLE_NOTARIZATION_KEY, APPLE_NOTARIZATION_KEY_ID, APPLE_NOTARIZATION_ISSUER_ID",
        ),
        (
            Secrets {
                notarization_issuer_id: None,
                ..secrets()
            },
            "APPLE_NOTARIZATION_ISSUER_ID",
        ),
    ] {
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &options(&config, &temp.path().join("out")),
            &secrets,
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(failure.step, "secrets");
        assert_eq!(
            failure.reason,
            format!("notarize = true needs {missing} in the environment")
        );
        assert!(failure.is_refusal());
        support::assert_only_metadata(&runner);
    }
}

#[test]
fn a_certificate_needs_its_password_and_valid_base64() {
    let temp = tempdir();
    let config = signed_config(temp.path(), false, false);
    for (secrets, reason) in [
        (
            Secrets {
                certificate_password: None,
                ..secrets()
            },
            "MACOS_CERTIFICATE is set without MACOS_CERTIFICATE_PASSWORD in the environment",
        ),
        (
            Secrets {
                certificate: Some("not base64 hunter2!".into()),
                ..secrets()
            },
            "MACOS_CERTIFICATE is not valid base64",
        ),
    ] {
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &options(&config, &temp.path().join("out")),
            &secrets,
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!((failure.step, failure.reason.as_str()), ("secrets", reason));
        support::assert_only_metadata(&runner);
    }
}

#[test]
fn a_failure_in_the_middle_deletes_the_keychain() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let mut runner = FakeRunner::new(temp.path());
    runner.fail = Some("codesign");
    let failure = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "sign");
    no_secret_in(&runner, &failure.reason);

    let lines = lines(&runner);
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines.last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()],
        "the clean-up deletes the keychain, which takes it out of the search list"
    );
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
    assert!(!Path::new(&keychain).parent().unwrap().exists());
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "no bundle and no temporary folder"
    );
}

#[test]
fn a_rejected_notarization_asks_for_its_log_and_stops() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, false);
    let mut runner = FakeRunner::new(temp.path());
    runner.notarization = "Invalid";
    let failure = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "notarize");
    assert!(
        failure.reason.contains("ended Invalid; its log is above"),
        "{}",
        failure.reason
    );
    let logs: Vec<&Recorded> = find(&runner, "xcrun", "notarytool")
        .into_iter()
        .filter(|command| command.args[1] == "log")
        .collect();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].args[2], "submission-1");
    assert_eq!(logs[0].args[3], "--key");
    assert_eq!(
        logs[0].args[5..],
        ["--key-id", "KEYID12345", "--issuer", "issuer-uuid"]
    );
    assert!(find(&runner, "xcrun", "stapler").is_empty());
    let lines = lines(&runner);
    assert_eq!(lines.last().unwrap()[..2], ["security", "delete-keychain"]);
    assert!(!out.join("Signed Lab.app").exists());
}

#[test]
fn an_empty_search_list_is_never_set() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    runner.no_keychains = true;
    let failure = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("keychain", "security list-keychains printed no keychain")
    );
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines(&runner)[4..],
        [
            vec!["security", "list-keychains", "-d", "user"],
            vec!["security", "delete-keychain", keychain.as_str()],
        ],
        "the search list is never set, and the keychain is deleted"
    );
}

#[test]
fn a_set_interrupt_flag_stops_at_the_next_step_and_still_cleans_up() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let interrupt = xtask::Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let during_codesign = interrupt.clone();
    runner.on_verify = Some(Box::new(move |_| during_codesign.set()));
    let failure =
        bundle_macos(&options(&config, &out), &secrets(), &interrupt, &mut runner).unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("notarize", "interrupted")
    );

    let lines = lines(&runner);
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines[lines.len() - 2][..2],
        ["codesign", "--verify"],
        "nothing runs after the interrupted step but the clean-up"
    );
    assert_eq!(
        lines.last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()],
        "the clean-up deletes the keychain, which takes it out of the search list"
    );
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
    assert!(!Path::new(&keychain).parent().unwrap().exists());
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "no bundle and no temporary folder"
    );
}

#[test]
fn an_interrupt_flag_set_before_the_stages_runs_nothing() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let interrupt = xtask::Interrupt::default();
    interrupt.set();
    let mut runner = FakeRunner::new(temp.path());
    let failure =
        bundle_macos(&options(&config, &out), &secrets(), &interrupt, &mut runner).unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("package", "interrupted")
    );
    assert!(runner.commands.is_empty(), "{:?}", runner.programs());
    assert!(!out.exists());
}

/// The keychain that `create-keychain` named.
fn keychain_of(runner: &FakeRunner) -> String {
    runner
        .commands
        .iter()
        .filter_map(|command| command.input.as_deref())
        .find_map(|input| input.strip_prefix("create-keychain -p \""))
        .unwrap()
        .split('"')
        .nth(2)
        .unwrap()
        .to_owned()
}

#[test]
fn a_panic_still_cleans_up_then_goes_on() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let mut runner = FakeRunner::new(temp.path());
    runner.on_verify = Some(Box::new(|_| panic!("a bug while signing")));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bundle_macos(
            &options(&config, &out),
            &secrets(),
            &xtask::Interrupt::default(),
            &mut runner,
        )
    }));
    let panic = panicked.expect_err("the panic goes on after the clean-up");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"a bug while signing"));
    let keychain = keychain_of(&runner);
    let lines = lines(&runner);
    assert_eq!(
        lines.last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()],
        "the clean-up deletes the keychain, which takes it out of the search list"
    );
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
    assert!(!Path::new(&keychain).parent().unwrap().exists());
    let staging: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with(".bundle-macos."))
        .collect();
    assert!(
        staging.is_empty(),
        "the staging folder is removed: {staging:?}"
    );
}

#[test]
fn the_clean_up_keeps_keychains_added_during_the_run() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    let search_list = runner.search_list.clone();
    let added = "/Users/test/Library/Keychains/added.keychain-db";
    runner.on_verify = Some(Box::new(move |_| {
        search_list.lock().unwrap().push(added.to_owned());
    }));
    bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    assert_eq!(
        *runner.search_list.lock().unwrap(),
        [LOGIN_KEYCHAIN, added],
        "only the temporary keychain is taken out"
    );
}

#[test]
fn a_keychain_never_made_is_not_a_clean_up_failure() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    // `create-keychain` is stopped before it makes the keychain, and
    // `delete-keychain` then finds none.
    runner.fail = Some("security -i");
    runner.also_fail = Some("security delete-keychain");
    let failure = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("keychain", "security exited with exit status: 1")
    );
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines(&runner).last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()],
        "the keychain is marked as made before create-keychain runs"
    );
    assert!(!Path::new(&keychain).parent().unwrap().exists());
}

#[test]
fn a_password_that_security_would_split_is_refused() {
    let temp = tempdir();
    let config = signed_config(temp.path(), false, false);
    for password in ["pass\nword", "pass\rword", "pass\0word"] {
        let mut runner = FakeRunner::new(temp.path());
        let failure = bundle_macos(
            &options(&config, &temp.path().join("out")),
            &Secrets {
                certificate_password: Some(password.into()),
                ..secrets()
            },
            &xtask::Interrupt::default(),
            &mut runner,
        )
        .unwrap_err();
        assert_eq!(
            (failure.step, failure.reason.as_str()),
            (
                "secrets",
                "MACOS_CERTIFICATE_PASSWORD holds a line break or a NUL, which `security -i` cannot take"
            )
        );
        support::assert_only_metadata(&runner);
    }
}

#[test]
fn a_step_failing_while_the_interrupt_flag_is_set_is_an_interrupt() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), true, true);
    let interrupt = xtask::Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    // Ctrl-C reaches `codesign` too, which dies of it.
    runner.fail = Some("codesign");
    let signal = interrupt.clone();
    runner.on_fail = Some(Box::new(move || signal.set()));
    let failure =
        bundle_macos(&options(&config, &out), &secrets(), &interrupt, &mut runner).unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("sign", "codesign exited with exit status: 1")
    );
    assert!(interrupt.is_set(), "so xtask exits 130");
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines(&runner).last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()]
    );
    assert_eq!(*runner.search_list.lock().unwrap(), [LOGIN_KEYCHAIN]);
}

#[test]
fn an_over_long_security_command_is_refused_naming_its_longest_value() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    let password = "p".repeat(5000);
    let failure = bundle_macos(
        &options(&config, &out),
        &Secrets {
            certificate_password: Some(password.clone()),
            ..secrets()
        },
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        (
            "keychain",
            "a `security -i` command must be shorter than 4096 bytes; \
             its longest value is MACOS_CERTIFICATE_PASSWORD"
        )
    );
    assert!(
        !runner
            .commands
            .iter()
            .filter_map(|command| command.input.as_deref())
            .any(|input| input.starts_with("import ")),
        "the import never runs"
    );
    assert!(runner.certificate_seen.is_none());
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines(&runner).last().unwrap(),
        &["security", "delete-keychain", keychain.as_str()]
    );
}

#[test]
fn an_interrupt_flag_set_during_the_last_step_publishes_nothing() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = config_in(temp.path(), "Lab", "0.1.0", "test.yard.late", "yardlate");
    let interrupt = xtask::Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let signal = interrupt.clone();
    runner.on_verify = Some(Box::new(move |_| signal.set()));
    let failure = bundle_macos(
        &options(&config, &out),
        &Secrets::default(),
        &interrupt,
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("publish", "interrupted")
    );
    assert_eq!(interrupt.exit_code(), 130);
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "no app, and no staging folder"
    );
}

/// Writes a previous app, zip, and disk image of `Signed Lab` into `out`,
/// and returns what `out` holds.
fn previous_release(out: &Path) -> Vec<(String, Vec<u8>)> {
    std::fs::create_dir_all(out.join("Signed Lab.app")).unwrap();
    std::fs::write(out.join("Signed Lab.app/old"), b"old app").unwrap();
    std::fs::write(out.join("Signed Lab-0.3.0.zip"), b"old zip").unwrap();
    std::fs::write(out.join("Signed Lab-0.3.0.dmg"), b"old dmg").unwrap();
    contents(out)
}

/// Every file under `folder`, with its bytes, in order.
fn contents(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_owned()];
    while let Some(next) = folders.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let name = path.strip_prefix(folder).unwrap().display().to_string();
                files.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn an_interrupt_flag_set_during_the_disk_images_notarization_publishes_nothing() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let before = previous_release(&out);
    let config = signed_config(temp.path(), true, true);
    let interrupt = xtask::Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let signal = interrupt.clone();
    runner.on_command = Some(Box::new(move |command| {
        if command.program == "xcrun"
            && command.args[1] == "submit"
            && command.args[2].ends_with("release.dmg")
        {
            signal.set();
        }
    }));
    let options = MacosArgs {
        zip: true,
        ..options(&config, &out)
    };
    let failure = bundle_macos(&options, &secrets(), &interrupt, &mut runner).unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("staple", "interrupted")
    );
    assert_eq!(interrupt.exit_code(), 130);
    assert_eq!(contents(&out), before, "out is as it was before the run");
}

#[test]
fn a_failed_later_publish_puts_every_previous_file_back() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let before = previous_release(&out);
    let config = signed_config(temp.path(), false, true);
    let mut runner = FakeRunner::new(temp.path());
    // The staged disk image disappears once it is hashed, so publishing it
    // fails after the app and the zip were published.
    runner.on_command = Some(Box::new(|command| {
        if command.program == "shasum" && command.args[2].ends_with("release.dmg") {
            std::fs::remove_file(&command.args[2]).unwrap();
        }
    }));
    let options = MacosArgs {
        zip: true,
        ..options(&config, &out)
    };
    let failure = bundle_macos(
        &options,
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(failure.step, "dmg");
    assert!(
        failure.reason.starts_with("could not move "),
        "{}",
        failure.reason
    );
    assert_eq!(contents(&out), before, "out is as it was before the run");
}

#[test]
fn a_search_list_with_a_broken_line_is_never_set() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let mut runner = FakeRunner::new(temp.path());
    runner
        .search_list
        .lock()
        .unwrap()
        .push("/Users/test/Library/Keychains/line\nbreak.keychain-db".to_owned());
    let failure = bundle_macos(
        &options(&config, &out),
        &secrets(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        (
            "keychain",
            "security list-keychains printed a line that is not one quoted path, \
             as a keychain path with a line break or a double quote prints; \
             the search list is left as it is"
        )
    );
    let keychain = keychain_of(&runner);
    assert_eq!(
        lines(&runner)[4..],
        [
            vec!["security", "list-keychains", "-d", "user"],
            vec!["security", "delete-keychain", keychain.as_str()],
        ],
        "the list is never set, and the keychain is deleted"
    );
}

#[test]
fn an_interrupt_flag_set_during_the_clean_up_still_makes_the_run_an_interrupt() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = signed_config(temp.path(), false, false);
    let interrupt = xtask::Interrupt::default();
    let mut runner = FakeRunner::new(temp.path());
    let signal = interrupt.clone();
    runner.on_command = Some(Box::new(move |command| {
        if command.program == "security" && command.args[0] == "delete-keychain" {
            signal.set();
        }
    }));
    let failure =
        bundle_macos(&options(&config, &out), &secrets(), &interrupt, &mut runner).unwrap_err();
    assert_eq!(
        (failure.step, failure.reason.as_str()),
        ("clean-up", "interrupted")
    );
    assert_eq!(interrupt.exit_code(), 130);
}

#[test]
fn an_ad_hoc_signature_carries_the_entitlements() {
    let temp = tempdir();
    let out = temp.path().join("out");
    let config = config_in(temp.path(), "Lab", "0.1.0", "test.yard.adhoc", "yardadhoc");
    std::fs::copy(
        support::under_test("signed").with_file_name("entitlements.plist"),
        temp.path().join("entitlements.plist"),
    )
    .unwrap();
    let text = std::fs::read_to_string(&config).unwrap()
        + "[release.macos]\nentitlements = \"entitlements.plist\"\n";
    std::fs::write(&config, text).unwrap();
    let mut runner = FakeRunner::new(temp.path());
    bundle_macos(
        &options(&config, &out),
        &Secrets::default(),
        &xtask::Interrupt::default(),
        &mut runner,
    )
    .unwrap();
    let entitlements = temp
        .path()
        .join("entitlements.plist")
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    let signature = find(&runner, "codesign", "--force");
    assert_eq!(signature.len(), 1);
    assert_eq!(
        signature[0].args[..6],
        [
            "--force",
            "--deep",
            "--entitlements",
            entitlements.as_str(),
            "--sign",
            "-"
        ]
    );
}
