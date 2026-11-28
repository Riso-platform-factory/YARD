use yard_i18n::negotiate;

#[test]
fn regional_tag_falls_back_to_parent_language() {
    assert_eq!(negotiate(Some("vi-VN"), None, &["en", "vi"]), "vi");
    assert_eq!(negotiate(None, Some("vi-VN"), &["en", "vi"]), "vi");
}

#[test]
fn unsupported_language_falls_back_to_english() {
    assert_eq!(negotiate(Some("fr"), None, &["en", "vi"]), "en");
    assert_eq!(negotiate(None, Some("fr"), &["en", "vi"]), "en");
}

#[test]
fn posix_and_empty_locales_fall_back_to_english() {
    for tag in ["C", "POSIX", ""] {
        assert_eq!(
            negotiate(None, Some(tag), &["en", "vi"]),
            "en",
            "system locale {tag:?} must count as no language"
        );
        assert_eq!(
            negotiate(Some(tag), None, &["en", "vi"]),
            "en",
            "requested locale {tag:?} must fall back to English"
        );
    }
    assert_eq!(negotiate(None, None, &["en", "vi"]), "en");
}

#[test]
fn posix_encoding_suffix_falls_back_to_english() {
    assert_eq!(negotiate(Some("C.UTF-8"), None, &["en", "vi"]), "en");
    assert_eq!(negotiate(None, Some("C.UTF-8"), &["en", "vi"]), "en");
}

#[test]
fn underscore_encoding_and_mixed_case_tags_resolve_to_parent_language() {
    for tag in ["vi_VN", "vi_VN.UTF-8", "VI-vn"] {
        assert_eq!(
            negotiate(Some(tag), None, &["en", "vi"]),
            "vi",
            "requested tag {tag:?} must resolve to vi"
        );
    }
    assert_eq!(
        negotiate(None, Some("vi_VN"), &["en", "vi"]),
        "vi",
        "system locale \"vi_VN\" must resolve to vi"
    );
}

#[test]
fn setting_beats_system_locale() {
    assert_eq!(negotiate(Some("vi"), Some("en"), &["en", "vi"]), "vi");
    assert_eq!(negotiate(Some("en"), Some("vi"), &["en", "vi"]), "en");
    assert_eq!(negotiate(Some("vi-VN"), Some("en-US"), &["en", "vi"]), "vi");
}

#[test]
fn regional_english_falls_back_to_en() {
    assert_eq!(negotiate(Some("en-GB"), None, &["en"]), "en");
    assert_eq!(negotiate(None, Some("en-GB"), &["en"]), "en");
}

#[test]
fn malformed_tag_falls_back_to_english() {
    for bad in ["not a locale!!!", "12345", "---", "en_US_$$$"] {
        assert_eq!(
            negotiate(Some(bad), None, &["en", "vi"]),
            "en",
            "malformed requested tag {bad:?} must fall back to English"
        );
        assert_eq!(
            negotiate(None, Some(bad), &["en", "vi"]),
            "en",
            "malformed system tag {bad:?} must fall back to English"
        );
    }
}

#[test]
fn posix_modifier_tags_resolve_and_c_modifier_falls_back_to_english() {
    for tag in ["vi_VN@euro", "vi_VN.UTF-8@euro"] {
        assert_eq!(
            negotiate(Some(tag), None, &["en", "vi"]),
            "vi",
            "requested tag {tag:?} must resolve to vi"
        );
        assert_eq!(
            negotiate(None, Some(tag), &["en", "vi"]),
            "vi",
            "system tag {tag:?} must resolve to vi"
        );
    }
    assert_eq!(negotiate(Some("C@euro"), None, &["en", "vi"]), "en");
    assert_eq!(negotiate(None, Some("C@euro"), &["en", "vi"]), "en");
}

#[test]
fn private_use_script_and_unicode_extension_tags_walk_parents() {
    assert_eq!(
        negotiate(Some("vi-x-a-b"), None, &["en", "vi", "vi-x-a"]),
        "vi-x-a"
    );
    assert_eq!(negotiate(Some("vi-x-a-b"), None, &["en", "vi"]), "vi");
    assert_eq!(
        negotiate(Some("zh-Hant-TW"), None, &["en", "zh", "zh-Hant"]),
        "zh-Hant"
    );
    assert_eq!(
        negotiate(Some("sr-Latn-RS"), None, &["en", "sr", "sr-Latn"]),
        "sr-Latn"
    );
    assert_eq!(negotiate(Some("en-US-u-ca-gregory"), None, &["en"]), "en");
}
