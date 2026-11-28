use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_settings::Settings;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-editable-settings-{}-{}-{nanos:x}",
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

fn settings_file(home: &TempDir, content: &str) -> PathBuf {
    let config = home.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let path = config.join("settings.json");
    fs::write(&path, content).unwrap();
    path
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

fn assert_unregistered<T: Settings>(cx: &gpui::App) {
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = yard_settings::get::<T>(cx);
    }))
    .expect_err("get of an unregistered section must panic");
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

#[gpui::test]
fn mismatched_extension_name_is_refused_as_register_settings_refuses_it(cx: &mut TestAppContext) {
    let home = TempDir::new();
    let path = settings_file(
        &home,
        r#"{"sample": {"preview": true, "sort": "date", "page_size": 25}}"#,
    );

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let guest = yard_shell::Extension::new(cx, "guest").unwrap();
        guest.register_editable_settings::<Sample>(cx);
        assert_unregistered::<Sample>(cx);

        let sample = yard_shell::Extension::new(cx, "sample").unwrap();
        sample.register_settings::<Sample>(cx);
    });

    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<Sample>(cx),
            &Sample {
                preview: true,
                sort: Order::Date,
                page_size: 25,
                ..Sample::default()
            }
        );
    });
}

#[gpui::test]
fn matching_extension_name_describes_the_section_while_refused_one_does_not(
    cx: &mut TestAppContext,
) {
    let home = TempDir::new();
    let path = settings_file(
        &home,
        r#"{"sample": {"preview": true, "sort": "date", "page_size": 25}}"#,
    );

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let guest = yard_shell::Extension::new(cx, "guest").unwrap();
        guest.register_editable_settings::<Sample>(cx);
        assert_unregistered::<Sample>(cx);
        assert!(
            yard_settings::described_sections(cx).is_empty(),
            "refused registration must not describe the section"
        );

        let sample = yard_shell::Extension::new(cx, "sample").unwrap();
        sample.register_editable_settings::<Sample>(cx);
    });

    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<Sample>(cx),
            &Sample {
                preview: true,
                sort: Order::Date,
                page_size: 25,
                ..Sample::default()
            }
        );
        let described = yard_settings::described_sections(cx);
        assert_eq!(described.len(), 1);
        assert_eq!(described[0].key, "sample");
        assert_eq!(described[0].title.as_ref(), "Test section");
        assert!(
            yard_settings::json_schema(cx)["properties"]
                .get("sample")
                .is_some()
        );
    });
}
