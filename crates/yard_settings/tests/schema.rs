use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_settings::{
    SchemaWrite, Settings, describe, described_sections, init, json_schema, load, register,
    write_schema_file,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-schema-{}-{}-{nanos:x}",
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Name,
    Date,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Advanced {
    flag: bool,
}

/// Test section
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    /// Show the preview.
    preview: bool,
    /// Sort order.
    sort: Order,
    /// Items per page.
    #[schemars(range(min = 1, max = 100))]
    page_size: u32,
    /// Greeting.
    greeting: String,
    /// Nested.
    advanced: Advanced,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            preview: false,
            sort: Order::Name,
            page_size: 10,
            greeting: "hi".into(),
            advanced: Advanced::default(),
        }
    }
}

impl Settings for Sample {
    const KEY: &'static str = "sample";
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Plain {
    enabled: bool,
}

impl Settings for Plain {
    const KEY: &'static str = "plain";
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Untitled {
    enabled: bool,
}

impl Settings for Untitled {
    const KEY: &'static str = "untitled";
}

fn resolve_subschema<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(name) = reference.strip_prefix("#/definitions/")
    {
        return &root["definitions"][name];
    }
    if let Some(first) = schema
        .get("allOf")
        .and_then(Value::as_array)
        .filter(|items| items.len() == 1)
        .and_then(|items| items.first())
    {
        return resolve_subschema(root, first);
    }
    schema
}

#[gpui::test]
fn json_schema_describes_sample_properties_doc_comments_and_draft_07_root(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");

    cx.update(|cx| {
        init(cx);
        load(cx, settings_path);
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let root = cx.read(json_schema);

    assert_eq!(
        root.get("$schema").and_then(Value::as_str),
        Some("http://json-schema.org/draft-07/schema#"),
        "expected exact Draft 07 meta-schema URI, got {root:#}"
    );
    assert!(
        root.get("$defs").is_none(),
        "Draft 07 uses definitions or inline subschemas, not $defs: {root:#}"
    );
    assert!(
        root.get("unevaluatedProperties").is_none(),
        "Draft 07 must not contain unevaluatedProperties: {root:#}"
    );

    assert_eq!(root["type"], "object");
    assert_eq!(
        root["properties"]["$schema"]["type"], "string",
        "$schema property must be allowed as a string: {root:#}"
    );
    match root.get("additionalProperties") {
        None | Some(Value::Bool(true)) => {}
        Some(Value::Object(map)) if map.is_empty() => {}
        other => panic!(
            "unknown top-level keys must validate, got additionalProperties={other:?} in {root:#}"
        ),
    }
    assert!(
        root.get("propertyNames").is_none(),
        "root schema must not restrict top-level property names: {root:#}"
    );
    assert!(
        root.get("required")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "root schema must not require any top-level keys: {root:#}"
    );

    let sample_raw = &root["properties"]["sample"];
    let sample = resolve_subschema(&root, sample_raw);
    assert_eq!(sample["type"], "object");
    let section_title_in_schema = sample_raw
        .get("title")
        .or_else(|| sample.get("title"))
        .and_then(Value::as_str);
    let section_desc_in_schema = sample_raw
        .get("description")
        .or_else(|| sample.get("description"))
        .and_then(Value::as_str);
    assert!(
        section_title_in_schema == Some("Test section")
            || section_desc_in_schema == Some("Test section"),
        "expected section doc comment 'Test section' in schema, got {sample_raw:#}"
    );

    let props = sample["properties"]
        .as_object()
        .expect("sample schema has properties");

    let preview_raw = &props["preview"];
    let preview = resolve_subschema(&root, preview_raw);
    assert_eq!(preview["type"], "boolean");
    assert_eq!(
        preview_raw
            .get("description")
            .or_else(|| preview.get("description"))
            .and_then(Value::as_str),
        Some("Show the preview.")
    );

    let sort_raw = &props["sort"];
    let sort = resolve_subschema(&root, sort_raw);
    assert_eq!(sort["type"], "string");
    assert_eq!(sort["enum"], json!(["name", "date"]));
    assert_eq!(
        sort_raw
            .get("description")
            .or_else(|| sort.get("description"))
            .and_then(Value::as_str),
        Some("Sort order.")
    );

    let page_size_raw = &props["page_size"];
    let page_size = resolve_subschema(&root, page_size_raw);
    assert_eq!(page_size["type"], "integer");
    assert_eq!(page_size["minimum"].as_f64(), Some(1.0));
    assert_eq!(page_size["maximum"].as_f64(), Some(100.0));
    assert_eq!(
        page_size_raw
            .get("description")
            .or_else(|| page_size.get("description"))
            .and_then(Value::as_str),
        Some("Items per page.")
    );

    let greeting_raw = &props["greeting"];
    let greeting = resolve_subschema(&root, greeting_raw);
    assert_eq!(greeting["type"], "string");
    assert_eq!(
        greeting_raw
            .get("description")
            .or_else(|| greeting.get("description"))
            .and_then(Value::as_str),
        Some("Greeting.")
    );

    let advanced_raw = &props["advanced"];
    let advanced = resolve_subschema(&root, advanced_raw);
    assert_eq!(advanced["type"], "object");
    assert_eq!(
        advanced_raw
            .get("description")
            .or_else(|| advanced.get("description"))
            .and_then(Value::as_str),
        Some("Nested.")
    );
}

#[gpui::test]
fn write_schema_file_writes_then_reports_unchanged_with_identical_bytes(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    let schema_path = temp.path().join("settings.schema.json");

    cx.update(|cx| {
        init(cx);
        load(cx, settings_path);
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let first = cx
        .read(|cx| write_schema_file(cx, &schema_path))
        .expect("first schema write succeeds");
    assert_eq!(first, SchemaWrite::Written);

    fs::OpenOptions::new()
        .write(true)
        .open(&schema_path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .unwrap();
    let first_bytes = fs::read(&schema_path).unwrap();
    let first_mtime = fs::metadata(&schema_path).unwrap().modified().unwrap();
    let parsed: Value = serde_json::from_slice(&first_bytes).expect("written schema is valid JSON");
    assert_eq!(parsed, cx.read(json_schema));

    let second = cx
        .read(|cx| write_schema_file(cx, &schema_path))
        .expect("second schema write succeeds");
    assert_eq!(second, SchemaWrite::Unchanged);
    assert_eq!(fs::read(&schema_path).unwrap(), first_bytes);
    assert_eq!(
        fs::metadata(&schema_path).unwrap().modified().unwrap(),
        first_mtime
    );
}

#[gpui::test]
fn section_registered_without_describe_is_omitted_from_schema(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");

    cx.update(|cx| {
        init(cx);
        load(cx, settings_path);
        register::<Plain>(cx);
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let root = cx.read(json_schema);
    let props = root["properties"]
        .as_object()
        .expect("root schema has properties");
    assert!(props.contains_key("sample"));
    assert!(
        !props.contains_key("plain"),
        "undescribed section must not appear in json_schema: {root:#}"
    );

    let sections = cx.read(described_sections);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].key, "sample");
}

#[gpui::test]
fn described_sections_returns_key_title_schema_current_value_and_defaults(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(
        &settings_path,
        r#"{"sample": {"preview": true, "sort": "date", "page_size": 25}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, settings_path);
        register::<Sample>(cx);
        describe::<Sample>(cx);
        register::<Untitled>(cx);
        describe::<Untitled>(cx);
    });

    let sections = cx.read(described_sections);
    assert_eq!(sections.len(), 2);

    let sample = sections
        .iter()
        .find(|section| section.key == "sample")
        .expect("sample is in described_sections");
    assert_eq!(sample.title.as_ref(), "Test section");
    assert!(
        sample.schema["properties"]
            .as_object()
            .is_some_and(|props| {
                ["preview", "sort", "page_size", "greeting", "advanced"]
                    .into_iter()
                    .all(|field| props.contains_key(field))
            }),
        "described section schema must contain Sample properties: {:#}",
        sample.schema
    );
    assert_eq!(
        sample.value,
        serde_json::to_value(Sample {
            preview: true,
            sort: Order::Date,
            page_size: 25,
            greeting: "hi".into(),
            advanced: Advanced { flag: false },
        })
        .unwrap()
    );
    assert_eq!(
        sample.defaults,
        serde_json::to_value(Sample::default()).unwrap()
    );

    let untitled = sections
        .iter()
        .find(|section| section.key == "untitled")
        .expect("untitled is in described_sections");
    assert_eq!(untitled.title.as_ref(), "untitled");
}

fn default_shortcuts() -> HashMap<String, u32> {
    HashMap::from([
        ("zeta".into(), 1),
        ("alpha".into(), 2),
        ("mu".into(), 3),
        ("beta".into(), 4),
        ("omega".into(), 5),
        ("delta".into(), 6),
        ("kappa".into(), 7),
        ("epsilon".into(), 8),
    ])
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct MappedSection {
    #[serde(default = "default_shortcuts")]
    shortcuts: HashMap<String, u32>,
    zebra_flag: bool,
    alpha_flag: bool,
}

impl Default for MappedSection {
    fn default() -> Self {
        Self {
            shortcuts: default_shortcuts(),
            zebra_flag: true,
            alpha_flag: false,
        }
    }
}

impl Settings for MappedSection {
    const KEY: &'static str = "mapped";
}

fn assert_keys_sorted_at_every_level(value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            let mut sorted = keys.clone();
            sorted.sort_unstable();
            assert_eq!(
                keys, sorted,
                "object keys at {path} are not sorted: got {keys:?}, expected {sorted:?}"
            );
            for (key, child) in map {
                let child_path = format!("{path}.{key}");
                assert_keys_sorted_at_every_level(child, &child_path);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                assert_keys_sorted_at_every_level(child, &child_path);
            }
        }
        _ => {}
    }
}

#[gpui::test]
fn section_with_hashmap_default_writes_byte_identical_schema_files_over_20_generations(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let mut baseline: Option<Vec<u8>> = None;

    for run in 0..20 {
        let app = cx.new_app();
        let settings_path = temp.path().join(format!("settings-{run}.json"));
        let schema_path = temp.path().join(format!("settings-{run}.schema.json"));

        app.update(|cx| {
            init(cx);
            load(cx, settings_path);
            register::<Untitled>(cx);
            describe::<Untitled>(cx);
            register::<Sample>(cx);
            describe::<Sample>(cx);
            register::<MappedSection>(cx);
            describe::<MappedSection>(cx);
        });

        let outcome = app
            .read(|cx| write_schema_file(cx, &schema_path))
            .expect("schema write succeeds");
        assert_eq!(outcome, SchemaWrite::Written);

        let bytes = fs::read(&schema_path).unwrap();
        if let Some(expected) = &baseline {
            assert_eq!(
                &bytes,
                expected,
                "schema generation {run} differed from generation 0:\nrun {run}:\n{}\nrun 0:\n{}",
                String::from_utf8_lossy(&bytes),
                String::from_utf8_lossy(expected),
            );
        } else {
            baseline = Some(bytes);
        }
    }
}

#[gpui::test]
fn schema_keys_are_sorted_at_every_level(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");

    cx.update(|cx| {
        init(cx);
        load(cx, settings_path);
        // Register in non-alphabetical order to check section key ordering too.
        register::<Untitled>(cx);
        describe::<Untitled>(cx);
        register::<Sample>(cx);
        describe::<Sample>(cx);
        register::<MappedSection>(cx);
        describe::<MappedSection>(cx);
    });

    let root = cx.read(json_schema);
    assert_eq!(
        root.get("$schema").and_then(Value::as_str),
        Some("http://json-schema.org/draft-07/schema#")
    );
    assert_keys_sorted_at_every_level(&root, "$");
}
