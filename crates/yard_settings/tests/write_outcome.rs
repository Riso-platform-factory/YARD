use gpui::TestAppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_settings::{
    Settings, WriteOutcome, describe, get, init, load, register, reset, reset_value, write,
    write_value,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-write-outcome-{}-{}-{nanos:x}",
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

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct Sample {
    preview: bool,
    page_size: u32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            preview: false,
            page_size: 10,
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

fn assert_refused(outcome: WriteOutcome, forbidden_value: Option<&str>) {
    match outcome {
        WriteOutcome::Refused(reason) => {
            let formatted = format!("{reason:?}");
            assert!(!formatted.is_empty(), "refusal reason must not be empty");
            if let Some(secret) = forbidden_value {
                assert!(
                    !formatted.contains(secret),
                    "refusal reason leaked setting value {secret:?}: {formatted}"
                );
            }
        }
        other => panic!("expected WriteOutcome::Refused(_), got {other:?}"),
    }
}

#[gpui::test]
fn write_value_and_reset_value_report_written_unchanged_and_refusals(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Plain>(cx);
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    // Written, then Unchanged on the same value.
    let written = cx.update(|cx| write_value(cx, "sample", "page_size", json!(25)));
    assert_eq!(written, WriteOutcome::Written);
    cx.read(|cx| assert_eq!(get::<Sample>(cx).page_size, 25));

    let unchanged_write = cx.update(|cx| write_value(cx, "sample", "page_size", json!(25)));
    assert_eq!(unchanged_write, WriteOutcome::Unchanged);

    // Resetting an overridden field reports Written; resetting a missing field reports Unchanged.
    let reset_written = cx.update(|cx| reset_value(cx, "sample", "page_size"));
    assert_eq!(reset_written, WriteOutcome::Written);
    cx.read(|cx| assert_eq!(get::<Sample>(cx).page_size, 10));

    let reset_unchanged = cx.update(|cx| reset_value(cx, "sample", "page_size"));
    assert_eq!(reset_unchanged, WriteOutcome::Unchanged);

    let before = fs::read(&path).unwrap();

    // Unknown section.
    let unknown_section_write = cx.update(|cx| write_value(cx, "unknown", "page_size", json!(20)));
    assert_refused(unknown_section_write, None);
    let unknown_section_reset = cx.update(|cx| reset_value(cx, "unknown", "page_size"));
    assert_refused(unknown_section_reset, None);

    // Undescribed section (registered with plain `register`, not `describe`).
    let undescribed_write = cx.update(|cx| write_value(cx, "plain", "enabled", json!(true)));
    assert_refused(undescribed_write, None);
    let undescribed_reset = cx.update(|cx| reset_value(cx, "plain", "enabled"));
    assert_refused(undescribed_reset, None);

    // Unknown field.
    let unknown_field_write = cx.update(|cx| write_value(cx, "sample", "no_such_field", json!(1)));
    assert_refused(unknown_field_write, None);
    let unknown_field_reset = cx.update(|cx| reset_value(cx, "sample", "no_such_field"));
    assert_refused(unknown_field_reset, None);

    // Value that does not decode (string for `page_size: u32`).
    let sentinel = "not-a-u32-sentinel-value";
    let decode_refused = cx.update(|cx| write_value(cx, "sample", "page_size", json!(sentinel)));
    assert_refused(decode_refused, Some(sentinel));

    assert_eq!(fs::read(&path).unwrap(), before);
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                page_size: 10,
            }
        );
    });
}

#[gpui::test]
fn malformed_settings_json_refuses_write_value_and_reset_value_leaving_bytes_intact(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, r#"{"sample": {"preview": true}}"#).unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });

    let broken = b"{ not json";
    fs::write(&path, broken).unwrap();

    cx.update(|cx| write::<Sample>(cx, "page_size", json!(20)));
    assert_eq!(fs::read(&path).unwrap(), broken);

    cx.update(|cx| reset::<Sample>(cx, "preview"));
    assert_eq!(fs::read(&path).unwrap(), broken);

    let write_outcome = cx.update(|cx| write_value(cx, "sample", "page_size", json!(20)));
    assert_refused(write_outcome, Some("not json"));
    assert_eq!(fs::read(&path).unwrap(), broken);

    let reset_outcome = cx.update(|cx| reset_value(cx, "sample", "preview"));
    assert_refused(reset_outcome, Some("not json"));
    assert_eq!(fs::read(&path).unwrap(), broken);
}

#[gpui::test]
fn write_finding_value_already_on_disk_returns_unchanged_and_updates_store(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    fs::write(&path, "{}\n").unwrap();

    cx.update(|cx| {
        init(cx);
        load(cx, path.clone());
        register::<Sample>(cx);
        describe::<Sample>(cx);
    });
    cx.read(|cx| assert_eq!(get::<Sample>(cx), &Sample::default()));

    // External write after load with no watch tick.
    fs::write(&path, r#"{"sample": {"preview": true, "page_size": 42}}"#).unwrap();

    let outcome = cx.update(|cx| write_value(cx, "sample", "page_size", json!(42)));
    assert_eq!(outcome, WriteOutcome::Unchanged);
    cx.read(|cx| {
        assert_eq!(
            get::<Sample>(cx),
            &Sample {
                preview: true,
                page_size: 42,
            },
            "Unchanged write must still update the published section from disk"
        );
    });
}
