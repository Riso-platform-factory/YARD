#![cfg(feature = "dev")]

use gpui::TestAppContext;
use icu_locale_core::Locale;
use icu_plurals::{PluralCategory, PluralRules};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_i18n::{
    CatalogError, init, locale, register, set_system_locale_for_tests, t, t_count, t_with,
};
use yard_settings::WriteOutcome;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-i18n-catalog-{}-{}-{nanos:x}",
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

const APP_EN: &str = r#"{
  "close": "Close",
  "only_en": "English only",
  "update.installed": "{name} {version} is installed",
  "items": {
    "one": "{count} item",
    "other": "{count} items"
  }
}"#;

const APP_VI: &str = r#"{
  "close": "Đóng",
  "update.installed": "Đã cài {name} {version}",
  "items": {
    "one": "{count} mục (one)",
    "other": "{count} mục"
  }
}"#;

#[gpui::test]
fn language_section_is_editable_and_defaults_to_english_without_system_locale(
    cx: &mut TestAppContext,
) {
    let _dir = setup(cx);
    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert!(
            yard_settings::described_sections(cx)
                .iter()
                .any(|section| section.key == "language"),
            "init must register the `language` section as editable"
        );
    });
}

#[gpui::test]
fn lookup_arguments_plurals_and_fallbacks_follow_the_selected_language(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", APP_EN).unwrap();
        register(cx, "app", "vi", APP_VI).unwrap();
    });

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.close").as_str(), "Close");
        assert_eq!(
            t_with(
                cx,
                "app.update.installed",
                &[("name", "YARD"), ("version", "1.2.3")],
            )
            .as_str(),
            "YARD 1.2.3 is installed"
        );
        assert_eq!(t_count(cx, "app.items", 0).as_str(), "0 items");
        assert_eq!(t_count(cx, "app.items", 1).as_str(), "1 item");
        assert_eq!(t_count(cx, "app.items", 2).as_str(), "2 items");
        assert_eq!(t_count(cx, "app.items", 5).as_str(), "5 items");
    });

    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi")));
    assert_eq!(outcome, WriteOutcome::Written);

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "vi");
        assert_eq!(t(cx, "app.close").as_str(), "Đóng");
        assert_eq!(
            t_with(
                cx,
                "app.update.installed",
                &[("name", "YARD"), ("version", "1.2.3")],
            )
            .as_str(),
            "Đã cài YARD 1.2.3"
        );
        // Plural categories follow the CLDR data shipped in `icu_plurals`.
        let vi_locale: Locale = "vi".parse().unwrap();
        let vi_rules = PluralRules::try_new_cardinal((&vi_locale).into()).unwrap();
        for count in [0_u64, 1, 2, 5] {
            let expected = match vi_rules.category_for(count) {
                PluralCategory::One => format!("{count} mục (one)"),
                _ => format!("{count} mục"),
            };
            assert_eq!(t_count(cx, "app.items", count).as_str(), expected);
        }

        // Missing in `vi` falls back to `en`; missing everywhere returns the key.
        assert_eq!(t(cx, "app.only_en").as_str(), "English only");
        assert_eq!(t(cx, "app.missing").as_str(), "app.missing");
        assert_eq!(
            t_with(cx, "app.missing", &[("name", "YARD")]).as_str(),
            "app.missing"
        );
        assert_eq!(t_count(cx, "app.missing", 3).as_str(), "app.missing");
    });

    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("en")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.close").as_str(), "Close");
    });
}

#[gpui::test]
fn setting_overrides_system_locale_set_for_tests(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", APP_EN).unwrap();
        register(cx, "app", "vi", APP_VI).unwrap();
        set_system_locale_for_tests(cx, Some("vi-VN"));
    });

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "vi");
        assert_eq!(t(cx, "app.close").as_str(), "Đóng");
    });

    let outcome = cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("en")));
    assert_eq!(outcome, WriteOutcome::Written);
    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.close").as_str(), "Close");
    });
}

#[gpui::test]
fn malformed_catalogs_are_refused_and_register_nothing(cx: &mut TestAppContext) {
    let _dir = setup(cx);

    cx.update(|cx| {
        let not_object = register(cx, "app", "en", r#"["not", "an", "object"]"#)
            .expect_err("a JSON array must be refused");
        assert!(
            matches!(not_object, CatalogError::NotAnObject),
            "expected NotAnObject, got {not_object:?}"
        );

        let bad_value = register(cx, "app", "en", r#"{"good": "Kept?", "bad": 42}"#)
            .expect_err("a numeric catalog value must be refused");
        match bad_value {
            CatalogError::BadValue { key } => assert_eq!(key, "bad"),
            other => panic!("expected BadValue {{ key: \"bad\" }}, got {other:?}"),
        }
        assert_eq!(
            t(cx, "app.good").as_str(),
            "app.good",
            "no key from a catalog with BadValue may be registered"
        );

        let bad_plural_value = register(
            cx,
            "app",
            "en",
            r#"{"good": "Kept?", "items": {"other": 123}}"#,
        )
        .expect_err("a plural with a non-string branch must be refused");
        match bad_plural_value {
            CatalogError::BadValue { key } => assert_eq!(key, "items"),
            other => panic!("expected BadValue {{ key: \"items\" }}, got {other:?}"),
        }
        assert_eq!(
            t(cx, "app.good").as_str(),
            "app.good",
            "no key from a catalog with a malformed plural value may be registered"
        );

        let missing_other = register(
            cx,
            "app",
            "en",
            r#"{"good": "Kept?", "items": {"one": "{count} item"}}"#,
        )
        .expect_err("a plural without `other` must be refused");
        match missing_other {
            CatalogError::PluralWithoutOther { key } => assert_eq!(key, "items"),
            other => panic!("expected PluralWithoutOther {{ key: \"items\" }}, got {other:?}"),
        }
        assert_eq!(
            t(cx, "app.good").as_str(),
            "app.good",
            "no key from a catalog with PluralWithoutOther may be registered"
        );

        let bad_locale = register(cx, "app", "not a locale!!!", r#"{"good": "Kept?"}"#)
            .expect_err("a malformed locale tag must be refused");
        assert!(
            matches!(bad_locale, CatalogError::BadLocale),
            "expected BadLocale, got {bad_locale:?}"
        );
        assert_eq!(
            t(cx, "app.good").as_str(),
            "app.good",
            "no key from a catalog with BadLocale may be registered"
        );

        let empty_locale = register(cx, "app", "", r#"{"good": "Kept?"}"#)
            .expect_err("an empty locale tag must be refused");
        assert!(
            matches!(empty_locale, CatalogError::BadLocale),
            "expected BadLocale for empty locale, got {empty_locale:?}"
        );
    });
}

#[gpui::test]
fn key_without_namespace_dot_returns_the_key(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", APP_EN).unwrap();
    });

    cx.read(|cx| {
        assert_eq!(t(cx, "nodot").as_str(), "nodot");
        assert_eq!(t_with(cx, "nodot", &[("name", "YARD")]).as_str(), "nodot");
        assert_eq!(t_count(cx, "nodot", 1).as_str(), "nodot");
    });
}

#[gpui::test]
fn lookup_walks_parent_locales_before_english(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(
            cx,
            "yard_ui",
            "en",
            r#"{"x": "English X", "only_en": "UI English"}"#,
        )
        .unwrap();
        register(cx, "yard_ui", "vi", r#"{"x": "Tiếng Việt X"}"#).unwrap();
        register(
            cx,
            "ext",
            "en",
            r#"{"y": "English Y", "only_en": "Ext English"}"#,
        )
        .unwrap();
        register(cx, "ext", "vi-VN", r#"{"y": "Tiếng Việt Nam Y"}"#).unwrap();
    });

    let outcome =
        cx.update(|cx| yard_settings::write_value(cx, "language", "locale", json!("vi-VN")));
    assert_eq!(outcome, WriteOutcome::Written);

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "vi-VN");
        assert_eq!(t(cx, "yard_ui.x").as_str(), "Tiếng Việt X");
        assert_eq!(t(cx, "ext.y").as_str(), "Tiếng Việt Nam Y");
        assert_eq!(t(cx, "yard_ui.only_en").as_str(), "UI English");
        assert_eq!(t(cx, "ext.only_en").as_str(), "Ext English");
    });
}

#[gpui::test]
fn registering_again_replaces_and_malformed_replacement_keeps_old_catalog(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", r#"{"keep": "Old", "drop": "Gone"}"#).unwrap();
        assert_eq!(t(cx, "app.keep").as_str(), "Old");
        assert_eq!(t(cx, "app.drop").as_str(), "Gone");

        register(cx, "app", "EN", r#"{"keep": "New"}"#).unwrap();
        assert_eq!(t(cx, "app.keep").as_str(), "New");
        assert_eq!(
            t(cx, "app.drop").as_str(),
            "app.drop",
            "omitted key must be gone after replacement"
        );

        register(cx, "app", "en", r#"{"keep": 123}"#)
            .expect_err("malformed replacement must be refused");
        register(cx, "app", "en", r#"{"keep":"#)
            .expect_err("invalid JSON replacement must be refused");
        assert_eq!(
            t(cx, "app.keep").as_str(),
            "New",
            "previous catalog must stay after a malformed replacement"
        );
    });
}

#[gpui::test]
fn repeated_keys_at_top_level_or_inside_plural_are_refused(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", r#"{"existing": "Kept"}"#).unwrap();

        register(cx, "app", "en", r#"{"dup": "first", "dup": "second"}"#)
            .expect_err("repeated top-level key must be refused");
        register(cx, "app", "en", r#"{"dup": 42, "dup": "valid"}"#)
            .expect_err("repeated top-level key hiding invalid value must be refused");
        register(
            cx,
            "app",
            "en",
            r#"{"items": {"other": "first", "other": "second"}}"#,
        )
        .expect_err("repeated plural branch key must be refused");
        register(
            cx,
            "app",
            "en",
            r#"{"items": {"one": 42, "one": "one", "other": "other"}}"#,
        )
        .expect_err("repeated plural branch key hiding invalid value must be refused");

        assert_eq!(t(cx, "app.existing").as_str(), "Kept");
        assert_eq!(t(cx, "app.dup").as_str(), "app.dup");
        assert_eq!(t(cx, "app.items").as_str(), "app.items");
    });
}

#[gpui::test]
fn empty_namespace_dotted_namespace_and_empty_key_are_refused(cx: &mut TestAppContext) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "", "en", r#"{"x": "X"}"#).expect_err("empty namespace must be refused");
        register(cx, "bad.ns", "en", r#"{"x": "X"}"#)
            .expect_err("namespace containing a dot must be refused");
        register(cx, "app", "en", r#"{"": "Empty", "ok": "Ok"}"#)
            .expect_err("empty key must be refused");

        assert_eq!(t(cx, "app.ok").as_str(), "app.ok");
        assert_eq!(t(cx, "bad.ns.x").as_str(), "bad.ns.x");
    });
}

#[gpui::test]
fn register_and_t_work_before_init_in_english(cx: &mut TestAppContext) {
    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.hello").as_str(), "app.hello");
    });

    cx.update(|cx| {
        register(cx, "app", "en", r#"{"hello": "Hello"}"#).unwrap();
        register(cx, "app", "vi", r#"{"hello": "Xin chào"}"#).unwrap();
    });

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.hello").as_str(), "Hello");
    });

    let dir = TempDir::new();
    let path = dir.path().join("settings.json");
    fs::write(&path, r#"{"language": {"locale": "vi"}}"#).unwrap();
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        init(cx);
    });

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "vi");
        assert_eq!(t(cx, "app.hello").as_str(), "Xin chào");
    });
}

#[gpui::test]
fn init_under_test_app_reads_no_host_language(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.path().join("settings.json");
    fs::write(&path, "{}").unwrap();
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        init(cx);

        register(cx, "app", "en", r#"{"hello": "English"}"#).unwrap();
        register(cx, "app", "vi", r#"{"hello": "Tiếng Việt"}"#).unwrap();
        register(cx, "app", "en-US", r#"{"hello": "US English"}"#).unwrap();

        if let Some(host) = sys_locale::get_locale() {
            let normalized = host
                .split(['.', '@'])
                .next()
                .unwrap_or(&host)
                .replace('_', "-");
            if let Ok(parsed) = normalized.parse::<Locale>() {
                let tag = parsed.to_string();
                if tag != "en" {
                    let _ = register(cx, "app", &tag, r#"{"hello": "Host"}"#);
                }
                let language = parsed.id.language.as_str();
                if language != "en" && language != tag {
                    let _ = register(cx, "app", language, r#"{"hello": "Host Language"}"#);
                }
            }
        }
    });

    cx.read(|cx| {
        assert_eq!(locale(cx).as_str(), "en");
        assert_eq!(t(cx, "app.hello").as_str(), "English");
    });
}

#[gpui::test]
fn register_refuses_non_bcp47_locale_tags_and_keeps_existing_english_catalog(
    cx: &mut TestAppContext,
) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(cx, "app", "en", r#"{"hello": "English"}"#).unwrap();

        for bad_tag in ["en.", "en.@!", "en.unrelated!", "vi_VN"] {
            let Err(err) = register(cx, "app", bad_tag, r#"{"hello": "Hijacked"}"#) else {
                panic!("non-BCP-47 catalog tag {bad_tag:?} must be refused");
            };
            assert!(
                matches!(err, CatalogError::BadLocale),
                "expected BadLocale for {bad_tag:?}, got {err:?}"
            );
            assert_eq!(
                t(cx, "app.hello").as_str(),
                "English",
                "registered en catalog must stay unchanged after refusing {bad_tag:?}"
            );
        }
    });
}

#[gpui::test]
fn t_with_keeps_unknown_placeholders_unmatched_braces_and_argument_braces_literal(
    cx: &mut TestAppContext,
) {
    let _dir = setup(cx);
    cx.update(|cx| {
        register(
            cx,
            "app",
            "en",
            r#"{
                "unknown_only": "Hello {unknown}",
                "unknown_and_known": "Hello {unknown} and {name}",
                "unclosed": "Open {name",
                "trailing_open": "{name} {",
                "stray_close": "Close } {name}",
                "nested_open": "prefix { still {name}",
                "double_braces": "{{name}}",
                "two_args": "{a} / {b}",
                "self_arg": "{x}"
            }"#,
        )
        .unwrap();
    });

    cx.read(|cx| {
        assert_eq!(
            t_with(cx, "app.unknown_only", &[("name", "YARD")]).as_str(),
            "Hello {unknown}"
        );
        assert_eq!(
            t_with(cx, "app.unknown_and_known", &[("name", "YARD")]).as_str(),
            "Hello {unknown} and YARD"
        );
        assert_eq!(
            t_with(cx, "app.unclosed", &[("name", "YARD")]).as_str(),
            "Open {name"
        );
        assert_eq!(
            t_with(cx, "app.trailing_open", &[("name", "YARD")]).as_str(),
            "YARD {"
        );
        assert_eq!(
            t_with(cx, "app.stray_close", &[("name", "YARD")]).as_str(),
            "Close } YARD"
        );
        assert_eq!(
            t_with(cx, "app.nested_open", &[("name", "YARD")]).as_str(),
            "prefix { still YARD"
        );
        assert_eq!(
            t_with(cx, "app.double_braces", &[("name", "YARD")]).as_str(),
            "{YARD}"
        );
        assert_eq!(
            t_with(cx, "app.two_args", &[("a", "{b}"), ("b", "B")]).as_str(),
            "{b} / B"
        );
        assert_eq!(t_with(cx, "app.self_arg", &[("x", "{x}")]).as_str(), "{x}");
    });
}
