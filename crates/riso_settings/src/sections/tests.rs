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

fn json(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap()
}

fn changed_map(
    applied: &[(&'static str, Option<serde_json::Value>)],
    content: &serde_json::Map<String, serde_json::Value>,
) -> std::collections::BTreeMap<&'static str, Option<serde_json::Value>> {
    changed_sections(applied, content).into_iter().collect()
}

fn observe_counter<T: Settings>(
    cx: &mut gpui::App,
) -> (std::rc::Rc<std::cell::Cell<usize>>, gpui::Subscription) {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let counted = calls.clone();
    let subscription = observe::<T>(cx, move |_| counted.set(counted.get() + 1));
    (calls, subscription)
}

fn tick(cx: &mut gpui::TestAppContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
}

#[test]
fn parse_content_reads_a_json_object() {
    assert_eq!(parse_content(b"{}"), Ok(serde_json::Map::new()));

    let raw: &[u8] = br#"{"first": {"greeting": "hi"}, "undeclared": 1}"#;
    assert_eq!(
        parse_content(raw),
        Ok(parse_object(
            r#"{"first": {"greeting": "hi"}, "undeclared": 1}"#
        ))
    );
}

#[test]
fn parse_content_rejects_invalid_json() {
    for bad in ["not json", r#"{"first": "#] {
        let reason = parse_content(bad.as_bytes()).expect_err("invalid JSON must fail");
        assert!(!reason.is_empty(), "empty reason for {bad}");
    }
    let reason = parse_content(&b"\xff"[..]).expect_err("invalid UTF-8 must fail");
    assert!(!reason.is_empty());
}

#[test]
fn parse_content_rejects_json_that_is_not_an_object() {
    for raw in ["[1, 2, 3]", "[]", "null", "42", r#""text""#, "true"] {
        let reason = parse_content(raw.as_bytes()).expect_err("non-object JSON must fail");
        assert!(!reason.is_empty(), "empty reason for {raw}");
    }
}

#[test]
fn changed_sections_reports_nothing_when_contents_match() {
    let applied = [
        ("first", Some(json(r#"{"greeting": "hi"}"#))),
        ("second", None),
    ];
    let content = parse_object(r#"{"first": {"greeting": "hi"}}"#);
    assert!(changed_sections(&applied, &content).is_empty());
}

#[test]
fn changed_sections_reports_changed_and_new_content() {
    let applied = [
        ("first", Some(json(r#"{"greeting": "hi"}"#))),
        ("second", None),
    ];
    let content = parse_object(r#"{"first": {"greeting": "yo"}, "second": {"label": "x"}}"#);
    assert_eq!(
        changed_map(&applied, &content),
        std::collections::BTreeMap::from([
            ("first", Some(json(r#"{"greeting": "yo"}"#))),
            ("second", Some(json(r#"{"label": "x"}"#))),
        ])
    );
}

#[test]
fn changed_sections_reports_a_removed_key_as_none() {
    let applied = [("first", Some(json(r#"{"greeting": "hi"}"#)))];
    let content = parse_object(r#"{"second": {"label": "x"}}"#);
    assert_eq!(changed_sections(&applied, &content), vec![("first", None)]);
}

#[test]
fn changed_sections_reports_a_section_that_became_unreadable() {
    let applied = [("first", Some(json(r#"{"greeting": "hi"}"#)))];
    let content = parse_object(r#"{"first": []}"#);
    assert_eq!(
        changed_sections(&applied, &content),
        vec![("first", Some(json(r#"[]"#)))]
    );
}

#[test]
fn changed_sections_sees_through_spaces_and_key_order() {
    let applied = [(
        "first",
        Some(json(r#"{"a": 1, "b": {"x": true, "y": [1, 2]}}"#)),
    )];
    let content = parse_object(r#"{"first": { "b" : { "y" : [ 1 , 2 ], "x" : true }, "a" : 1 }}"#);
    assert!(changed_sections(&applied, &content).is_empty());
}

#[test]
fn changed_sections_ignores_keys_that_no_section_declares() {
    let applied = [("first", Some(json(r#"{"greeting": "hi"}"#)))];
    let content = parse_object(r#"{"undeclared": {"new": true}, "first": {"greeting": "hi"}}"#);
    assert!(changed_sections(&applied, &content).is_empty());
}

#[test]
fn changed_sections_reports_only_the_sections_that_changed() {
    let applied = [
        ("first", Some(json(r#"{"greeting": "hi"}"#))),
        ("second", Some(json(r#"{"label": "a"}"#))),
    ];
    let content = parse_object(r#"{"first": {"greeting": "hi"}, "second": {"label": "b"}}"#);
    assert_eq!(
        changed_sections(&applied, &content),
        vec![("second", Some(json(r#"{"label": "b"}"#)))]
    );
}

#[gpui::test]
fn an_edit_reaches_get_and_calls_observe_once(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"greeting": "file", "size": 20}}"#).unwrap();
    let (calls, _subscription) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        let observed = observe_counter::<First>(cx);
        watch(cx);
        observed
    });
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "file".into(),
                size: 20,
                enabled: true,
            }
        );
    });

    tick(cx);
    assert_eq!(calls.get(), 0);

    std::fs::write(&path, r#"{"first": {"greeting": "hi", "size": 20}}"#).unwrap();
    tick(cx);
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "hi".into(),
                size: 20,
                enabled: true,
            }
        );
    });
    assert_eq!(calls.get(), 1);

    std::fs::write(&path, r#"{"first": {"greeting": "again", "size": 20}}"#).unwrap();
    tick(cx);
    cx.read(|cx| assert_eq!(get::<First>(cx).greeting, "again"));
    assert_eq!(calls.get(), 2);
}

#[gpui::test]
fn an_edit_of_another_key_does_not_call_observe(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "new"}}"#,
    )
    .unwrap();
    tick(cx);

    cx.read(|cx| {
        assert_eq!(get::<First>(cx).greeting, "file");
        assert_eq!(get::<Second>(cx).label, "new");
    });
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 1);
}

#[gpui::test]
fn a_broken_file_keeps_the_values_and_calls_nothing(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    for broken in ["not json", "[1, 2, 3]"] {
        std::fs::write(&path, broken).unwrap();
        tick(cx);
        cx.read(|cx| {
            assert_eq!(get::<First>(cx).greeting, "file");
            assert_eq!(get::<Second>(cx).label, "saved");
        });
        assert_eq!(first_calls.get(), 0);
        assert_eq!(second_calls.get(), 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }
}

#[gpui::test]
fn a_broken_section_keeps_its_value_while_another_section_updates(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    std::fs::write(
        &path,
        r#"{"first": {"size": "big"}, "second": {"label": "new"}}"#,
    )
    .unwrap();
    tick(cx);

    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "file".into(),
                size: 20,
                enabled: true,
            }
        );
        assert_eq!(
            get::<Second>(cx),
            &Second {
                label: "new".into(),
                limit: 3,
            }
        );
    });
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 1);
}

#[gpui::test]
fn a_broken_section_counts_as_seen_until_its_content_changes(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    // The first section breaks; the second is valid.
    std::fs::write(
        &path,
        r#"{"first": {"size": "big"}, "second": {"label": "new"}}"#,
    )
    .unwrap();
    tick(cx);
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "file".into(),
                size: 20,
                enabled: true,
            }
        );
        assert_eq!(get::<Second>(cx).label, "new");
    });
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 1);

    // The same broken content counts as seen while the second keeps changing.
    for label in ["newer", "newest"] {
        std::fs::write(
            &path,
            format!(r#"{{"first": {{"size": "big"}}, "second": {{"label": "{label}"}}}}"#),
        )
        .unwrap();
        tick(cx);
        cx.read(|cx| {
            assert_eq!(
                get::<First>(cx),
                &First {
                    greeting: "file".into(),
                    size: 20,
                    enabled: true,
                }
            );
            assert_eq!(get::<Second>(cx).label, label);
        });
        assert_eq!(first_calls.get(), 0);
    }
    assert_eq!(second_calls.get(), 3);

    // Back to a readable value is a change: the broken content was seen.
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "newest"}}"#,
    )
    .unwrap();
    tick(cx);
    cx.read(|cx| {
        assert_eq!(
            get::<First>(cx),
            &First {
                greeting: "file".into(),
                size: 20,
                enabled: true,
            }
        );
        assert_eq!(get::<Second>(cx).label, "newest");
    });
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 3);
}

#[gpui::test]
fn a_deleted_file_gives_defaults_and_calls_observe(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"greeting": "file", "size": 20}}"#).unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    std::fs::remove_file(&path).unwrap();
    tick(cx);

    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
        assert_eq!(get::<Second>(cx), &Second::default());
    });
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0);
}

#[gpui::test]
fn a_removed_key_gives_the_section_its_defaults(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    std::fs::write(&path, r#"{"second": {"label": "saved"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| {
        assert_eq!(get::<First>(cx), &First::default());
        assert_eq!(get::<Second>(cx).label, "saved");
    });
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 0);
}

#[gpui::test]
fn an_edit_with_the_same_content_calls_nothing(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"first": {"greeting": "file", "size": 20}, "second": {"label": "saved"}}"#,
    )
    .unwrap();
    let ((first_calls, _first), (second_calls, _second)) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        register::<Second>(cx);
        let first = observe_counter::<First>(cx);
        let second = observe_counter::<Second>(cx);
        watch(cx);
        (first, second)
    });

    std::fs::write(
        &path,
        r#"{"second" : { "label" : "saved" }, "first" : { "size" : 20 , "greeting" : "file" }}"#,
    )
    .unwrap();
    tick(cx);

    cx.read(|cx| {
        assert_eq!(get::<First>(cx).greeting, "file");
        assert_eq!(get::<Second>(cx).label, "saved");
    });
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 0);
}

#[gpui::test]
fn watch_twice_starts_one_check(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"greeting": "file"}}"#).unwrap();
    let (calls, _subscription) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        let observed = observe_counter::<First>(cx);
        watch(cx);
        watch(cx);
        observed
    });

    std::fs::write(&path, r#"{"first": {"greeting": "hi"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(get::<First>(cx).greeting, "hi"));
    assert_eq!(calls.get(), 1);
}

#[gpui::test]
fn dropping_the_subscription_stops_the_calls(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"greeting": "file"}}"#).unwrap();
    let (calls, subscription) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        let observed = observe_counter::<First>(cx);
        watch(cx);
        observed
    });
    drop(subscription);

    std::fs::write(&path, r#"{"first": {"greeting": "hi"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(get::<First>(cx).greeting, "hi"));
    assert_eq!(calls.get(), 0);
}

#[gpui::test]
fn watch_reads_the_file_as_load_left_it(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    std::fs::write(&path, r#"{"first": {"greeting": "file"}}"#).unwrap();
    let (calls, _subscription) = cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<First>(cx);
        // The file changes after `load`, before `watch`.
        std::fs::write(&path, r#"{"first": {"greeting": "between"}}"#).unwrap();
        let observed = observe_counter::<First>(cx);
        watch(cx);
        observed
    });

    tick(cx);

    cx.read(|cx| assert_eq!(get::<First>(cx).greeting, "between"));
    assert_eq!(calls.get(), 1);
}
