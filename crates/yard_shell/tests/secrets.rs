#![cfg(feature = "dev")]

use gpui::TestAppContext;
use yard_shell::{self as shell, Extension, MAX_SECRET_BYTES, SecretError, SecretStore};

#[gpui::test]
async fn set_get_overwrite_delete_and_missing_keys(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    assert_eq!(MAX_SECRET_BYTES, 2560);

    let store = SecretStore::memory();
    let app = cx.new_app();
    let notes = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "notes").unwrap()
    });

    // Missing key: get is Ok(None), delete is Ok(()).
    let missing = app.read(|cx| notes.get_secret("api-token", cx)).await;
    assert!(
        matches!(missing, Ok(None)),
        "get of a missing key must be Ok(None), got {missing:?}"
    );
    let delete_missing = app.read(|cx| notes.delete_secret("api-token", cx)).await;
    assert_eq!(
        delete_missing,
        Ok(()),
        "delete of a missing key must be Ok(())"
    );

    // Set then get returns the stored bytes.
    app.read(|cx| notes.set_secret("api-token", b"first-secret", cx))
        .await
        .unwrap();
    let first = app
        .read(|cx| notes.get_secret("api-token", cx))
        .await
        .unwrap()
        .expect("entry must exist after set");
    assert_eq!(first.expose(), b"first-secret");
    assert_eq!(first.expose_str(), Ok("first-secret"));

    // Overwrite then get returns the new bytes.
    app.read(|cx| notes.set_secret("api-token", b"second-secret", cx))
        .await
        .unwrap();
    let second = app
        .read(|cx| notes.get_secret("api-token", cx))
        .await
        .unwrap()
        .expect("entry must exist after overwrite");
    assert_eq!(second.expose(), b"second-secret");
    assert_eq!(second.expose_str(), Ok("second-secret"));

    // Delete then get returns Ok(None).
    app.read(|cx| notes.delete_secret("api-token", cx))
        .await
        .unwrap();
    let after_delete = app.read(|cx| notes.get_secret("api-token", cx)).await;
    assert!(
        matches!(after_delete, Ok(None)),
        "get after delete must be Ok(None), got {after_delete:?}"
    );

    app.quit();
    app.run_until_parked();
}

#[gpui::test]
async fn two_extensions_are_isolated_for_the_same_key(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let store = SecretStore::memory();
    let app = cx.new_app();
    let (alpha, beta) = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        let alpha = Extension::new(cx, "alpha").unwrap();
        let beta = Extension::new(cx, "beta").unwrap();
        (alpha, beta)
    });

    app.read(|cx| alpha.set_secret("shared-key", b"alpha-only", cx))
        .await
        .unwrap();

    let beta_before = app.read(|cx| beta.get_secret("shared-key", cx)).await;
    assert!(
        matches!(beta_before, Ok(None)),
        "beta must not see alpha's secret, got {beta_before:?}"
    );

    app.read(|cx| beta.set_secret("shared-key", b"beta-only", cx))
        .await
        .unwrap();

    let alpha_secret = app
        .read(|cx| alpha.get_secret("shared-key", cx))
        .await
        .unwrap()
        .expect("alpha's secret must remain intact");
    assert_eq!(alpha_secret.expose(), b"alpha-only");

    let beta_secret = app
        .read(|cx| beta.get_secret("shared-key", cx))
        .await
        .unwrap()
        .expect("beta's secret must be stored under beta");
    assert_eq!(beta_secret.expose(), b"beta-only");

    // Deleting in alpha must not affect beta.
    app.read(|cx| alpha.delete_secret("shared-key", cx))
        .await
        .unwrap();
    let alpha_after = app.read(|cx| alpha.get_secret("shared-key", cx)).await;
    assert!(matches!(alpha_after, Ok(None)));
    let beta_after = app
        .read(|cx| beta.get_secret("shared-key", cx))
        .await
        .unwrap()
        .expect("deleting alpha's entry must not remove beta's");
    assert_eq!(beta_after.expose(), b"beta-only");

    app.quit();
    app.run_until_parked();
}

#[gpui::test]
async fn boundary_values_and_allowed_128_byte_key_round_trip(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let store = SecretStore::memory();
    let app = cx.new_app();
    let ext = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "notes").unwrap()
    });

    // Empty value round-trips.
    app.read(|cx| ext.set_secret("empty-value", b"", cx))
        .await
        .unwrap();
    let empty = app
        .read(|cx| ext.get_secret("empty-value", cx))
        .await
        .unwrap()
        .expect("empty secret must be stored as Some");
    assert_eq!(empty.expose(), b"");
    assert_eq!(empty.expose_str(), Ok(""));

    // Exactly MAX_SECRET_BYTES is accepted.
    let max_bytes = vec![0x5Au8; MAX_SECRET_BYTES];
    app.read(|cx| ext.set_secret("max-value", &max_bytes, cx))
        .await
        .unwrap();
    let loaded_max = app
        .read(|cx| ext.get_secret("max-value", cx))
        .await
        .unwrap()
        .expect("MAX_SECRET_BYTES secret must be stored");
    assert_eq!(loaded_max.expose(), max_bytes.as_slice());

    // Lowercase keys with digits and `. _ -` round-trip, including a 128-byte key.
    let alphabet = "abcdefghijklmnopqrstuvwxyz0123456789._-";
    let max_key: String = alphabet.chars().cycle().take(128).collect();
    assert_eq!(max_key.len(), 128);
    for allowed_key in ["api-token", "a.b_c-1", max_key.as_str()] {
        app.read(|cx| ext.set_secret(allowed_key, b"fits", cx))
            .await
            .unwrap();
        let loaded_key = app
            .read(|cx| ext.get_secret(allowed_key, cx))
            .await
            .unwrap()
            .expect("allowed lowercase key must round-trip");
        assert_eq!(loaded_key.expose(), b"fits");
        app.read(|cx| ext.delete_secret(allowed_key, cx))
            .await
            .unwrap();
    }

    // Non-UTF-8 bytes round-trip through `expose`, and `expose_str` is Err.
    let non_utf8 = [0xFFu8, 0xFE, 0x80, 0x00];
    app.read(|cx| ext.set_secret("binary-secret", &non_utf8, cx))
        .await
        .unwrap();
    let binary = app
        .read(|cx| ext.get_secret("binary-secret", cx))
        .await
        .unwrap()
        .expect("binary secret must round-trip");
    assert_eq!(binary.expose(), &non_utf8);
    assert!(
        binary.expose_str().is_err(),
        "expose_str on non-UTF-8 bytes must return Err"
    );

    app.quit();
    app.run_until_parked();
}

#[gpui::test]
async fn refusals_are_checked_before_reaching_the_store(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let store = SecretStore::memory();
    let app = cx.new_app();
    let ext = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "notes").unwrap()
    });

    // Oversized value (MAX_SECRET_BYTES + 1) is TooLarge without consuming fail_next.
    let too_large = vec![b'x'; MAX_SECRET_BYTES + 1];
    store.fail_next(SecretError::Failed);
    let result = app
        .read(|cx| ext.set_secret("valid-key", &too_large, cx))
        .await;
    assert_eq!(
        result,
        Err(SecretError::TooLarge),
        "MAX_SECRET_BYTES + 1 must be refused with TooLarge"
    );
    let armed = app.read(|cx| ext.get_secret("valid-key", cx)).await;
    assert_eq!(
        armed.err(),
        Some(SecretError::Failed),
        "fail_next must still be pending after TooLarge refusal"
    );
    assert!(matches!(
        app.read(|cx| ext.get_secret("valid-key", cx)).await,
        Ok(None)
    ));

    // Invalid keys are refused before the store on set, get, and delete.
    let key_129 = "a".repeat(129);
    let invalid_keys = [
        "",
        key_129.as_str(),
        "bad key",
        "a/b",
        "é",
        "a\n",
        "API-Token",
        "Key",
        "api-tOken",
    ];

    for bad_key in invalid_keys {
        // set_secret
        store.fail_next(SecretError::Failed);
        assert_eq!(
            app.read(|cx| ext.set_secret(bad_key, b"val", cx)).await,
            Err(SecretError::InvalidKey),
            "set_secret must refuse invalid key {bad_key:?}"
        );
        assert_eq!(
            app.read(|cx| ext.get_secret("valid-key", cx)).await.err(),
            Some(SecretError::Failed),
            "fail_next must stay pending after set_secret InvalidKey for {bad_key:?}"
        );

        // get_secret
        store.fail_next(SecretError::Failed);
        assert_eq!(
            app.read(|cx| ext.get_secret(bad_key, cx)).await.err(),
            Some(SecretError::InvalidKey),
            "get_secret must refuse invalid key {bad_key:?}"
        );
        assert_eq!(
            app.read(|cx| ext.get_secret("valid-key", cx)).await.err(),
            Some(SecretError::Failed),
            "fail_next must stay pending after get_secret InvalidKey for {bad_key:?}"
        );

        // delete_secret
        store.fail_next(SecretError::Failed);
        assert_eq!(
            app.read(|cx| ext.delete_secret(bad_key, cx)).await,
            Err(SecretError::InvalidKey),
            "delete_secret must refuse invalid key {bad_key:?}"
        );
        assert_eq!(
            app.read(|cx| ext.get_secret("valid-key", cx)).await.err(),
            Some(SecretError::Failed),
            "fail_next must stay pending after delete_secret InvalidKey for {bad_key:?}"
        );
    }

    app.quit();
    app.run_until_parked();
}

#[gpui::test]
async fn fail_next_injects_each_store_error_once_then_recovers(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let store = SecretStore::memory();
    let app = cx.new_app();
    let ext = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "notes").unwrap()
    });

    app.read(|cx| ext.set_secret("token", b"initial", cx))
        .await
        .unwrap();

    for expected in [
        SecretError::Unavailable,
        SecretError::Denied,
        SecretError::Failed,
    ] {
        // Fails the next set_secret and does not overwrite the stored value.
        store.fail_next(expected);
        assert_eq!(
            app.read(|cx| ext.set_secret("token", b"should-not-write", cx))
                .await,
            Err(expected),
            "fail_next({expected:?}) must fail set_secret"
        );
        let after_set_fail = app
            .read(|cx| ext.get_secret("token", cx))
            .await
            .unwrap()
            .expect("call after failed set_secret must succeed");
        assert_eq!(after_set_fail.expose(), b"initial");

        // Fails the next get_secret, then the call after works.
        store.fail_next(expected);
        assert_eq!(
            app.read(|cx| ext.get_secret("token", cx)).await.err(),
            Some(expected),
            "fail_next({expected:?}) must fail get_secret"
        );
        let after_get_fail = app
            .read(|cx| ext.get_secret("token", cx))
            .await
            .unwrap()
            .expect("call after failed get_secret must succeed");
        assert_eq!(after_get_fail.expose(), b"initial");

        // Fails the next delete_secret without deleting, then a follow-up call works.
        store.fail_next(expected);
        assert_eq!(
            app.read(|cx| ext.delete_secret("token", cx)).await,
            Err(expected),
            "fail_next({expected:?}) must fail delete_secret"
        );
        let still_there = app
            .read(|cx| ext.get_secret("token", cx))
            .await
            .unwrap()
            .expect("failed delete_secret must leave the entry intact");
        assert_eq!(still_there.expose(), b"initial");
    }

    app.quit();
    app.run_until_parked();
}

#[gpui::test]
async fn debug_and_error_display_never_leak_key_or_value(cx: &mut TestAppContext) {
    cx.executor().allow_parking();

    let key = "user-token-xyz";
    let value = "s3cr3t-value";

    let store = SecretStore::memory();
    let app = cx.new_app();
    let ext = app.update(|cx| {
        shell::set_secret_store(cx, store.clone());
        Extension::new(cx, "notes").unwrap()
    });

    app.read(|cx| ext.set_secret(key, value.as_bytes(), cx))
        .await
        .unwrap();
    let secret = app
        .read(|cx| ext.get_secret(key, cx))
        .await
        .unwrap()
        .expect("secret must exist");

    let debug_output = format!("{secret:?}");
    assert_eq!(debug_output, "Secret(<redacted>)");
    assert!(
        !debug_output.contains(value),
        "Secret Debug must not contain the value: {debug_output}"
    );
    assert!(
        !debug_output.contains(key),
        "Secret Debug must not contain the key: {debug_output}"
    );

    let errors = [
        (SecretError::InvalidKey, "invalid secret key"),
        (SecretError::TooLarge, "secret too large"),
        (SecretError::Unavailable, "secret store unavailable"),
        (SecretError::Denied, "access to the secret store denied"),
        (SecretError::Failed, "secret store failed"),
    ];
    for (error, expected_text) in errors {
        let displayed = error.to_string();
        assert_eq!(displayed, expected_text);
        assert!(
            !displayed.contains(key),
            "SecretError Display must not contain the key: {displayed}"
        );
        assert!(
            !displayed.contains(value),
            "SecretError Display must not contain the value: {displayed}"
        );
    }

    app.quit();
    app.run_until_parked();
}
