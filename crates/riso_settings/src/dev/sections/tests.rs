use super::*;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-settings-sections-{}-{}-{nanos:x}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct First {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for First {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for First {
    const KEY: &'static str = "first";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Second {
    label: String,
    limit: usize,
}

impl Default for Second {
    fn default() -> Self {
        Self {
            label: "panel".into(),
            limit: 3,
        }
    }
}

impl Settings for Second {
    const KEY: &'static str = "second";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct SameKeyAsFirst {
    count: u32,
}

impl Default for SameKeyAsFirst {
    fn default() -> Self {
        Self { count: 99 }
    }
}

impl Settings for SameKeyAsFirst {
    const KEY: &'static str = First::KEY;
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Counted {
    call: u32,
}

impl Default for Counted {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        Self {
            call: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Settings for Counted {
    const KEY: &'static str = "counted";
}

fn parse_object(text: &str) -> serde_json::Map<String, serde_json::Value> {
    let serde_json::Value::Object(map) = serde_json::from_str(text).unwrap() else {
        panic!("expected JSON object: {text}");
    };
    map
}

fn assert_get_unregistered_panics<T: Settings>(cx: &gpui::App) {
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = get::<T>(cx);
    }))
    .expect_err("get of unregistered section must panic");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&'static str>().copied())
        .expect("panic payload must be a string");
    let expected = format!(
        "section {} is read before it is registered",
        std::any::type_name::<T>()
    );
    assert_eq!(message, expected);
}

#[test]
fn read_file_missing_returns_none() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    assert_eq!(read_file(&path), Ok(None));
    assert!(!path.exists());
}

#[test]
fn read_file_reads_json_object() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");

    std::fs::write(&path, "{}").unwrap();
    assert_eq!(read_file(&path), Ok(Some(serde_json::Map::new())));

    let raw = r#"{"first": {"greeting": "hi", "size": 20}, "undeclared": 1}"#;
    std::fs::write(&path, raw).unwrap();
    assert_eq!(read_file(&path), Ok(Some(parse_object(raw))));
}

#[test]
fn read_file_rejects_unreadable_directory_path() {
    let temp = TempDir::new();
    let dir = temp.path().join("settings.json");
    std::fs::create_dir_all(&dir).unwrap();

    let reason = read_file(&dir).expect_err("directory path must fail to read");
    assert!(!reason.is_empty());
    assert!(dir.is_dir());
}

#[test]
fn read_file_rejects_invalid_json() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");

    for bad in ["not json", r#"{"first": "#] {
        std::fs::write(&path, bad).unwrap();
        let reason = read_file(&path).expect_err("invalid JSON must fail");
        assert!(!reason.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
}

#[test]
fn read_file_rejects_json_array_and_other_non_objects() {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");

    for non_object in ["[1, 2, 3]", "[]", "null", "42", r#""text""#, "true"] {
        std::fs::write(&path, non_object).unwrap();
        let reason = read_file(&path).expect_err("non-object JSON must fail");
        assert!(!reason.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), non_object);
    }
}

#[test]
fn decode_section_reads_present_and_partial_sections() {
    let full = parse_object(r#"{"first": {"greeting": "hi", "size": 20, "enabled": false}}"#);
    assert_eq!(
        decode_section::<First>(&full),
        Ok(Some(First {
            greeting: "hi".into(),
            size: 20,
            enabled: false,
        }))
    );

    let partial = parse_object(r#"{"first": {"greeting": "hi"}}"#);
    assert_eq!(
        decode_section::<First>(&partial),
        Ok(Some(First {
            greeting: "hi".into(),
            size: 14,
            enabled: true,
        }))
    );

    let empty_section = parse_object(r#"{"first": {}}"#);
    assert_eq!(
        decode_section::<First>(&empty_section),
        Ok(Some(First::default()))
    );
}

#[test]
fn decode_section_returns_none_when_absent() {
    assert_eq!(decode_section::<First>(&parse_object("{}")), Ok(None));
    assert_eq!(
        decode_section::<First>(&parse_object(r#"{"second": {"label": "custom"}}"#)),
        Ok(None)
    );
}

#[test]
fn decode_section_rejects_wrong_field_type() {
    let content = parse_object(r#"{"first": {"size": "big"}}"#);
    let reason = decode_section::<First>(&content).expect_err("wrong field type must fail");
    assert!(!reason.is_empty());
}

#[test]
fn decode_section_rejects_section_that_is_not_an_object() {
    for raw in [
        r#"{"first": []}"#,
        r#"{"first": ["hi", 20, false]}"#,
        r#"{"first": "hello"}"#,
        r#"{"first": 42}"#,
        r#"{"first": true}"#,
        r#"{"first": null}"#,
    ] {
        let content = parse_object(raw);
        let reason =
            decode_section::<First>(&content).expect_err("non-object section value must fail");
        assert!(!reason.is_empty(), "empty reason for {raw}");
    }
}

#[test]
fn decode_section_ignores_unknown_keys() {
    let only_unknown = parse_object(r#"{"undeclared": 1, "extra": [true, false]}"#);
    assert_eq!(decode_section::<First>(&only_unknown), Ok(None));

    let with_unknown = parse_object(r#"{"undeclared": 1, "first": {"enabled": false}}"#);
    assert_eq!(
        decode_section::<First>(&with_unknown),
        Ok(Some(First {
            enabled: false,
            ..First::default()
        }))
    );
}

#[gpui::test]
fn registered_section_reads_as_its_default(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
        register::<First>(cx);
    });
    cx.read(|cx| {
        let value = get::<First>(cx);
        let expected = First::default();
        assert_eq!(value.greeting, expected.greeting);
        assert_eq!(value.size, expected.size);
        assert_eq!(value.enabled, expected.enabled);
        assert_eq!(value, &expected);
    });
}

#[gpui::test]
fn two_sections_with_different_keys_are_read_independently(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        let first = get::<First>(cx);
        let second = get::<Second>(cx);
        assert_eq!(first.greeting, "hello");
        assert_eq!(first.size, 14);
        assert!(first.enabled);
        assert_eq!(second.label, "panel");
        assert_eq!(second.limit, 3);
        assert_eq!(first, &First::default());
        assert_eq!(second, &Second::default());
    });
}

#[gpui::test]
fn registering_same_type_twice_leaves_first_value(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
        register::<Counted>(cx);
        register::<Counted>(cx);
    });
    cx.read(|cx| {
        let value = get::<Counted>(cx);
        assert_eq!(value.call, 1);
        assert_ne!(value, &Counted::default());
    });
}

#[gpui::test]
fn registering_another_type_with_same_key_leaves_first_value(cx: &mut gpui::TestAppContext) {
    assert_eq!(SameKeyAsFirst::default().count, 99);
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
        register::<First>(cx);
        register::<SameKeyAsFirst>(cx);
    });
    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
    });
}

#[gpui::test]
fn registering_another_type_with_same_key_is_refused(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
        register::<First>(cx);
        register::<SameKeyAsFirst>(cx);
    });
    cx.read(|cx| {
        assert_get_unregistered_panics::<SameKeyAsFirst>(cx);
    });
}

#[gpui::test]
fn get_before_register_panics(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing);
    });
    cx.read(|cx| {
        assert_get_unregistered_panics::<First>(cx);
    });
}

#[gpui::test]
#[should_panic(expected = "riso_settings::init runs before a section is registered")]
fn register_before_init_panics(cx: &mut gpui::TestAppContext) {
    cx.update(register::<First>);
}

#[gpui::test]
#[should_panic(expected = "riso_settings::init runs once")]
fn second_init_panics(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        init(cx);
        init(cx);
    });
}

#[gpui::test]
fn loaded_values_reach_get_with_defaults_for_missing_fields(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"undeclared": 1, "first": {"greeting": "hi", "size": 20}, "second": {"limit": 9}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path);
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "hi".into(),
                size: 20,
                enabled: true,
            }
        );
        assert_eq!(
            get::<Second>(cx),
            &Second {
                label: "panel".into(),
                limit: 9,
            }
        );
    });
}

#[gpui::test]
fn absent_section_gets_defaults_while_present_section_gets_file_values(
    cx: &mut gpui::TestAppContext,
) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"enabled": false}}"#).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path);
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "hello".into(),
                size: 14,
                enabled: false,
            }
        );
        assert_eq!(get::<Second>(cx), &Second::default());
    });
}

#[gpui::test]
fn broken_section_uses_defaults_without_affecting_other_sections(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let wrong_type_path = temp.path().join("wrong_type.json");
    let wrong_type_raw = r#"{"first": {"size": "big"}, "second": {"label": "saved", "limit": 7}}"#;
    std::fs::write(&wrong_type_path, wrong_type_raw).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, wrong_type_path.clone());
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
        assert_eq!(
            get::<Second>(cx),
            &Second {
                label: "saved".into(),
                limit: 7,
            }
        );
    });
    assert_eq!(
        std::fs::read_to_string(&wrong_type_path).unwrap(),
        wrong_type_raw
    );

    let non_object_path = temp.path().join("non_object.json");
    let non_object_raw =
        r#"{"first": {"greeting": "kept", "size": 18, "enabled": false}, "second": []}"#;
    std::fs::write(&non_object_path, non_object_raw).unwrap();

    let app = cx.new_app();
    app.update(|cx| {
        init(cx);
        load(cx, non_object_path.clone());
        register::<First>(cx);
        register::<Second>(cx);
    });
    app.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "kept".into(),
                size: 18,
                enabled: false,
            }
        );
        assert_eq!(get::<Second>(cx), &Second::default());
    });
    assert_eq!(
        std::fs::read_to_string(&non_object_path).unwrap(),
        non_object_raw
    );
}

#[gpui::test]
fn missing_or_invalid_file_gives_every_section_its_defaults(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let missing = temp.path().join("missing.json");
    cx.update(|cx| {
        init(cx);
        load(cx, missing.clone());
        register::<First>(cx);
        register::<Second>(cx);
    });
    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
        assert_eq!(get::<Second>(cx), &Second::default());
    });
    assert!(!missing.exists());

    for (name, bad) in [("invalid.json", "not json"), ("array.json", "[1, 2, 3]")] {
        let path = temp.path().join(name);
        std::fs::write(&path, bad).unwrap();
        let app = cx.new_app();
        app.update(|cx| {
            init(cx);
            load(cx, path.clone());
            register::<First>(cx);
            register::<Second>(cx);
        });
        app.read(|cx| {
            assert_eq!(get::<First>(cx), &First::default());
            assert_eq!(get::<Second>(cx), &Second::default());
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }

    let dir = temp.path().join("as_dir");
    std::fs::create_dir_all(&dir).unwrap();
    let app = cx.new_app();
    app.update(|cx| {
        init(cx);
        load(cx, dir.clone());
        register::<First>(cx);
        register::<Second>(cx);
    });
    app.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
        assert_eq!(get::<Second>(cx), &Second::default());
    });
    assert!(dir.is_dir());
}

#[gpui::test]
fn duplicate_registration_keeps_value_loaded_from_file(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "saved", "size": 32, "enabled": false, "count": 5}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path);
        register::<First>(cx);
        register::<First>(cx);
        register::<SameKeyAsFirst>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "saved".into(),
                size: 32,
                enabled: false,
            }
        );
        assert_get_unregistered_panics::<SameKeyAsFirst>(cx);
    });
}

#[gpui::test]
#[should_panic(expected = "riso_settings::load runs once")]
fn second_load_panics(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        load(cx, path);
    });
}
