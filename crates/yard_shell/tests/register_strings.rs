#![cfg(feature = "dev")]

use gpui::{App, TestAppContext};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_i18n::{CatalogError, init, register, set_system_locale_for_tests, t};
use yard_settings::WriteOutcome;
use yard_shell::Extension;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-shell-register-strings-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup(cx: &mut TestAppContext) -> TempDir {
    let dir = TempDir::new();
    let path = dir.path().join("settings.json");
    fs::write(&path, "{}").unwrap();
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        init(cx);
        set_system_locale_for_tests(cx, None);
    });
    dir
}

#[gpui::test]
fn extension_registers_strings_under_its_own_namespace(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        let notes = Extension::new(cx, "notes").unwrap();
        notes
            .register_strings(cx, "en", r#"{"title": "Notes"}"#)
            .unwrap();
        notes
            .register_strings(cx, "vi", r#"{"title": "Ghi chú"}"#)
            .unwrap();
    });

    cx.read(|cx| {
        assert_eq!(t(cx, "notes.title").as_str(), "Notes");
    });

    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi")));
    assert_eq!(outcome, WriteOutcome::Written);

    cx.read(|cx| {
        assert_eq!(t(cx, "notes.title").as_str(), "Ghi chú");
    });
}

#[gpui::test]
fn extension_cannot_register_into_another_namespace(cx: &mut TestAppContext) {
    // Compile-time check: the method takes only (cx, locale, json) and no namespace.
    let _: fn(&Extension, &mut App, &str, &str) -> Result<(), CatalogError> =
        Extension::register_strings;

    let _dir = setup(cx);
    cx.update(|cx| {
        let other = Extension::new(cx, "other").unwrap();
        other
            .register_strings(cx, "en", r#"{"title": "Other"}"#)
            .unwrap();

        let notes = Extension::new(cx, "notes").unwrap();
        notes
            .register_strings(cx, "en", r#"{"other.title": "Hijacked", "title": "Notes"}"#)
            .unwrap();
    });

    cx.read(|cx| {
        assert_eq!(t(cx, "other.title").as_str(), "Other");
        assert_eq!(t(cx, "notes.title").as_str(), "Notes");
        assert_eq!(t(cx, "notes.other.title").as_str(), "Hijacked");
    });
}

#[gpui::test]
fn extension_malformed_catalog_is_refused(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        let notes = Extension::new(cx, "notes").unwrap();
        let err = notes
            .register_strings(cx, "en", r#"{"good": "No", "bad": 42}"#)
            .expect_err("malformed extension catalog must be refused");
        match err {
            CatalogError::BadValue { key } => assert_eq!(key, "bad"),
            other => panic!("expected BadValue {{ key: \"bad\" }}, got {other:?}"),
        }
        assert_eq!(t(cx, "notes.good").as_str(), "notes.good");
    });
}

#[gpui::test]
fn extension_handles_for_reserved_namespaces_are_refused_and_keep_existing_catalogs(
    cx: &mut TestAppContext,
) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", r#"{"title": "App"}"#).unwrap();
        register(cx, "yard_ui", "en", r#"{"close": "Close"}"#).unwrap();
        register(cx, "yard_shell", "en", r#"{"new": "New Item"}"#).unwrap();

        for (namespace, key, original) in [
            ("app", "title", "App"),
            ("yard_ui", "close", "Close"),
            ("yard_shell", "new", "New Item"),
        ] {
            let ext = Extension::new(cx, namespace).unwrap();
            let err = ext
                .register_strings(cx, "en", &format!(r#"{{"{key}": "Hijacked"}}"#))
                .expect_err("reserved namespace must be refused");
            assert!(
                matches!(err, CatalogError::ReservedNamespace),
                "expected ReservedNamespace for {namespace:?}, got {err:?}"
            );
            assert_eq!(
                t(cx, &format!("{namespace}.{key}")).as_str(),
                original,
                "registered catalog for {namespace:?} must stay unchanged"
            );
        }

        let update = Extension::new(cx, "yard_update").unwrap();
        let err = update
            .register_strings(cx, "en", r#"{"ready": "Hijacked"}"#)
            .expect_err("yard_* reserved namespace must be refused");
        assert!(
            matches!(err, CatalogError::ReservedNamespace),
            "expected ReservedNamespace for yard_update, got {err:?}"
        );
        assert_eq!(t(cx, "yard_update.ready").as_str(), "yard_update.ready");
    });
}
