#![cfg(feature = "dev")]

use gpui::{
    AppContext, ClipboardItem, IntoElement, Pixels, Styled, TestAppContext, VisualTestContext,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use yard_settings::Settings;
use yard_shell::Extension;

#[cfg(target_os = "macos")]
const SELECT_ALL: &str = "cmd-a";
#[cfg(not(target_os = "macos"))]
const SELECT_ALL: &str = "ctrl-a";

#[cfg(target_os = "macos")]
const COPY: &str = "cmd-c";
#[cfg(not(target_os = "macos"))]
const COPY: &str = "ctrl-c";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-settings-editor-{}-{}-{nanos:x}",
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

struct Card;

impl gpui::Render for Card {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div().size_full()
    }
}

fn draw_frames(
    window: gpui::WindowHandle<yard_shell::Shell>,
    cx: &mut TestAppContext,
    frames: usize,
) {
    for _ in 0..frames {
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
                window.refresh();
                let _ = window.draw(cx);
            })
            .unwrap();
        cx.run_until_parked();
    }
}

fn click(visual: &mut VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} has bounds"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::default());
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn fields_vertical_span(visual: &mut VisualTestContext) -> Pixels {
    let selectors = [
        "field:sample.preview",
        "field:sample.sort",
        "field:sample.page_size",
        "field:sample.greeting",
        "field:sample.advanced",
    ];
    let mut top = gpui::px(f32::INFINITY);
    let mut bottom = gpui::px(f32::NEG_INFINITY);
    for selector in selectors {
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        top = top.min(bounds.top());
        bottom = bottom.max(bounds.bottom());
    }
    bottom - top
}

fn read_input_text(
    window: gpui::WindowHandle<yard_shell::Shell>,
    cx: &mut TestAppContext,
    visual: &mut VisualTestContext,
    selector: &'static str,
) -> String {
    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
    click(visual, selector);
    draw_frames(window, cx, 1);
    visual.simulate_keystrokes(SELECT_ALL);
    visual.simulate_keystrokes(COPY);
    draw_frames(window, cx, 1);
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

#[gpui::test]
fn settings_editor_edits_validates_resets_and_follows_settings_file(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(&settings_path, "{}\n").unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_settings::watch(cx);
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "sample").unwrap();
        sample.register_editable_settings::<Sample>(cx);
        sample.register(cx, "sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);

    // Open the editor with `settings_editor::Open`; the section is listed.
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    assert!(
        visual.update(|window, cx| yard_shell::has_modal(window, cx)),
        "settings_editor::Open opens the modal"
    );
    assert!(
        visual.debug_bounds("section:sample").is_some(),
        "the sample section is listed"
    );
    click(&mut visual, "section:sample");
    draw_frames(window, cx, 1);

    // `advanced` shows "Edit in settings.json".
    let advanced = visual
        .debug_bounds("field:sample.advanced")
        .expect("field:sample.advanced is drawn");
    assert!(
        advanced.size.width >= gpui::px(120.) && advanced.size.height > gpui::px(0.),
        "field:sample.advanced draws 'Edit in settings.json': {advanced:?}"
    );

    // Toggling `field:sample.preview` writes `true`.
    click(&mut visual, "field:sample.preview");
    draw_frames(window, cx, 2);
    assert_eq!(read_json(&settings_path)["sample"]["preview"], json!(true));
    cx.read(|cx| assert!(yard_settings::get::<Sample>(cx).preview));

    // Choosing `"date"` in `field:sample.sort` writes `"date"`.
    if visual.debug_bounds("MENU_ITEM-date").is_none() {
        click(&mut visual, "field:sample.sort");
        draw_frames(window, cx, 3);
    }
    click(&mut visual, "MENU_ITEM-date");
    draw_frames(window, cx, 3);
    assert_eq!(read_json(&settings_path)["sample"]["sort"], json!("date"));
    cx.read(|cx| assert_eq!(yard_settings::get::<Sample>(cx).sort, Order::Date));

    // Entering `"30"` in `field:sample.page_size` and confirming writes 30.
    click(&mut visual, "field:sample.page_size");
    draw_frames(window, cx, 1);
    visual.simulate_keystrokes(SELECT_ALL);
    visual.simulate_input("30");
    visual.simulate_keystrokes("enter");
    draw_frames(window, cx, 2);
    assert_eq!(read_json(&settings_path)["sample"]["page_size"], json!(30));
    cx.read(|cx| assert_eq!(yard_settings::get::<Sample>(cx).page_size, 30));
    assert!(
        visual.debug_bounds("reset:sample.page_size").is_some(),
        "modified page_size shows its reset button"
    );

    // Entering `"0"` writes nothing and shows a message.
    let before_bytes = fs::read(&settings_path).unwrap();
    let before_span = fields_vertical_span(&mut visual);
    click(&mut visual, "field:sample.page_size");
    draw_frames(window, cx, 1);
    visual.simulate_keystrokes(SELECT_ALL);
    visual.simulate_input("0");
    visual.simulate_keystrokes("enter");
    draw_frames(window, cx, 2);
    assert_eq!(fs::read(&settings_path).unwrap(), before_bytes);
    cx.read(|cx| assert_eq!(yard_settings::get::<Sample>(cx).page_size, 30));
    let after_span = fields_vertical_span(&mut visual);
    assert!(
        after_span > before_span,
        "refused '0' shows a message under page_size ({after_span:?} > {before_span:?})"
    );

    // `reset:sample.page_size` removes it.
    click(&mut visual, "reset:sample.page_size");
    draw_frames(window, cx, 2);
    assert!(
        read_json(&settings_path)["sample"]
            .get("page_size")
            .is_none(),
        "reset:sample.page_size removes page_size from settings.json"
    );
    cx.read(|cx| assert_eq!(yard_settings::get::<Sample>(cx).page_size, 10));
    assert!(
        visual.debug_bounds("reset:sample.page_size").is_none(),
        "reset button disappears once page_size returns to default"
    );
    assert_eq!(
        read_input_text(window, cx, &mut visual, "field:sample.page_size"),
        "10"
    );

    // Changing `settings.json` on disk updates the shown value.
    fs::write(
        &settings_path,
        r#"{
  "sample": {
    "preview": false,
    "sort": "name",
    "page_size": 45,
    "greeting": "from disk"
  }
}
"#,
    )
    .unwrap();
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    draw_frames(window, cx, 2);

    cx.read(|cx| {
        let sample = yard_settings::get::<Sample>(cx);
        assert_eq!(sample.page_size, 45);
        assert_eq!(sample.greeting, "from disk");
    });
    assert!(visual.debug_bounds("reset:sample.page_size").is_some());
    assert!(visual.debug_bounds("reset:sample.greeting").is_some());
    assert!(visual.debug_bounds("reset:sample.preview").is_none());
    assert!(visual.debug_bounds("reset:sample.sort").is_none());
    assert_eq!(
        read_input_text(window, cx, &mut visual, "field:sample.page_size"),
        "45"
    );
    assert_eq!(
        read_input_text(window, cx, &mut visual, "field:sample.greeting"),
        "from disk"
    );
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct NullableSection {
    nickname: Option<String>,
    keep: bool,
}

impl Settings for NullableSection {
    const KEY: &'static str = "nullable_sample";
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct ImpostorSample {
    preview: bool,
    extra: u32,
}

impl Settings for ImpostorSample {
    const KEY: &'static str = "sample";
}

#[gpui::test]
fn refused_write_from_switch_and_dropdown_shows_message_and_keeps_saved_value(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(&settings_path, r#"{"sample": {"sort": "date"}}"#).unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "sample").unwrap();
        sample.register_editable_settings::<Sample>(cx);
        sample.register(cx, "sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:sample");
    draw_frames(window, cx, 1);

    assert!(visual.debug_bounds("reset:sample.preview").is_none());
    assert!(visual.debug_bounds("reset:sample.sort").is_some());

    // Corrupt `settings.json` on disk so `write_value` refuses subsequent writes.
    let broken = b"{ not json";
    fs::write(&settings_path, broken).unwrap();

    // 1. Refused switch toggle shows a message under `preview` and keeps `false`.
    let sort_top_before = visual
        .debug_bounds("field:sample.sort")
        .expect("field:sample.sort is drawn")
        .top();
    click(&mut visual, "field:sample.preview");
    draw_frames(window, cx, 2);

    assert_eq!(fs::read(&settings_path).unwrap(), broken);
    cx.read(|cx| assert!(!yard_settings::get::<Sample>(cx).preview));
    assert!(
        visual.debug_bounds("reset:sample.preview").is_none(),
        "refused switch toggle must not mark preview modified"
    );
    let sort_top_after = visual
        .debug_bounds("field:sample.sort")
        .expect("field:sample.sort is drawn")
        .top();
    assert!(
        sort_top_after > sort_top_before,
        "refused switch toggle must show a message under preview ({sort_top_after:?} > {sort_top_before:?})"
    );

    // 2. Refused dropdown choice shows a message under `sort` and keeps saved `"date"`.
    let page_size_top_before = visual
        .debug_bounds("field:sample.page_size")
        .expect("field:sample.page_size is drawn")
        .top();
    if visual.debug_bounds("MENU_ITEM-name").is_none() {
        click(&mut visual, "field:sample.sort");
        draw_frames(window, cx, 3);
    }
    click(&mut visual, "MENU_ITEM-name");
    draw_frames(window, cx, 3);

    assert_eq!(fs::read(&settings_path).unwrap(), broken);
    cx.read(|cx| assert_eq!(yard_settings::get::<Sample>(cx).sort, Order::Date));
    assert!(
        visual.debug_bounds("reset:sample.sort").is_some(),
        "refused dropdown choice must keep saved 'date' value shown as modified"
    );
    let page_size_top_after = visual
        .debug_bounds("field:sample.page_size")
        .expect("field:sample.page_size is drawn")
        .top();
    assert!(
        page_size_top_after > page_size_top_before,
        "refused dropdown choice must show a message under sort ({page_size_top_after:?} > {page_size_top_before:?})"
    );
}

#[gpui::test]
fn clearing_nullable_text_field_resets_it(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(
        &settings_path,
        r#"{"nullable_sample": {"nickname": "alice", "keep": true}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "nullable_sample").unwrap();
        sample.register_editable_settings::<NullableSection>(cx);
        sample.register(cx, "nullable_sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "nullable_sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:nullable_sample");
    draw_frames(window, cx, 1);

    assert_eq!(
        read_input_text(window, cx, &mut visual, "field:nullable_sample.nickname"),
        "alice"
    );
    assert!(
        visual
            .debug_bounds("reset:nullable_sample.nickname")
            .is_some()
    );

    click(&mut visual, "field:nullable_sample.nickname");
    draw_frames(window, cx, 1);
    visual.simulate_keystrokes(SELECT_ALL);
    visual.simulate_keystrokes("backspace");
    visual.simulate_keystrokes("enter");
    draw_frames(window, cx, 2);

    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<NullableSection>(cx),
            &NullableSection {
                nickname: None,
                keep: true,
            }
        );
    });
    assert_eq!(
        read_json(&settings_path),
        json!({"nullable_sample": {"keep": true}})
    );
    assert!(
        visual
            .debug_bounds("reset:nullable_sample.nickname")
            .is_none()
    );
}

fn path_with_cur_dir_segment(dir: &Path, file_name: &str) -> PathBuf {
    let sep = std::path::MAIN_SEPARATOR;
    PathBuf::from(format!("{}{sep}.{sep}{file_name}", dir.display()))
}

#[gpui::test]
fn edit_in_settings_json_creates_empty_object_when_missing_and_keeps_concurrent_file(
    cx: &mut TestAppContext,
) {
    // `TestPlatform::open_with_system` is `unimplemented!()`, so an accepted
    // `ext.open_path` panics in `TestAppContext` (`extension-test-kit.md`).
    // A `.` segment resolves to the same file for `std::fs` while `open_path`
    // refuses it before calling `open_with_system`.
    let temp = TempDir::new();
    let real_path = temp.path().join("settings.json");
    let loaded_path = path_with_cur_dir_segment(temp.path(), "settings.json");
    assert!(!real_path.exists());

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, loaded_path);
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "sample").unwrap();
        sample.register_editable_settings::<Sample>(cx);
        sample.register(cx, "sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:sample");
    draw_frames(window, cx, 1);

    // 1. Missing `settings.json`: clicking "Edit in settings.json" creates `{}`.
    click(&mut visual, "field:sample.advanced");
    draw_frames(window, cx, 2);

    assert!(
        real_path.exists(),
        "Edit in settings.json must create settings.json when missing"
    );
    let created = fs::read_to_string(&real_path).unwrap();
    assert_eq!(created.trim(), "{}");
    assert_eq!(serde_json::from_str::<Value>(&created).unwrap(), json!({}));

    // 2. If `settings.json` was created just before clicking, it is not replaced.
    fs::remove_file(&real_path).unwrap();
    let existing = b"{\n  \"sample\": {\n    \"greeting\": \"created meanwhile\"\n  }\n}\n";
    fs::write(&real_path, existing).unwrap();

    click(&mut visual, "field:sample.advanced");
    draw_frames(window, cx, 2);

    assert_eq!(fs::read(&real_path).unwrap(), existing);
}

#[gpui::test]
fn describing_wrong_type_for_same_key_does_not_panic_editor(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(&settings_path, "{}\n").unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path);
        yard_settings::register::<Sample>(cx);
        yard_settings::describe::<ImpostorSample>(cx);

        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);
        editor.register(cx, "settings_editor.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        editor.place_root(cx, "root", "settings_editor.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);

    assert!(
        visual.update(|window, cx| yard_shell::has_modal(window, cx)),
        "editor modal opens without panicking"
    );
    assert!(visual.debug_bounds("section:sample").is_none());
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ThreeChoices {
    #[default]
    First,
    Second,
    Third,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct ChoiceSection {
    choice: ThreeChoices,
}

impl Settings for ChoiceSection {
    const KEY: &'static str = "choice_sample";
}

#[gpui::test]
fn escape_closes_open_dropdown_menu_and_keeps_settings_modal_open(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(&settings_path, "{}\n").unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path);
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "sample").unwrap();
        sample.register_editable_settings::<Sample>(cx);
        sample.register(cx, "sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:sample");
    draw_frames(window, cx, 1);

    click(&mut visual, "field:sample.sort");
    draw_frames(window, cx, 3);
    assert!(
        visual.debug_bounds("MENU_ITEM-name").is_some(),
        "dropdown menu opens before Escape"
    );
    assert!(visual.update(|window, cx| yard_shell::has_modal(window, cx)));

    visual.simulate_keystrokes("escape");
    draw_frames(window, cx, 2);

    assert!(
        visual.debug_bounds("MENU_ITEM-name").is_none(),
        "Escape closes the open dropdown menu"
    );
    assert!(
        visual.update(|window, cx| yard_shell::has_modal(window, cx)),
        "Escape on an open dropdown menu must leave the settings modal open"
    );
    assert!(visual.debug_bounds("field:sample.sort").is_some());
}

#[gpui::test]
fn up_down_and_enter_choose_dropdown_option_in_settings_editor(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(&settings_path, "{}\n").unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "choice_sample").unwrap();
        sample.register_editable_settings::<ChoiceSection>(cx);
        sample.register(cx, "choice_sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "choice_sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:choice_sample");
    draw_frames(window, cx, 1);

    // From `first` (0): Down -> `second` (1), Down -> `third` (2), Up -> `second` (1), Enter selects `second`.
    click(&mut visual, "field:choice_sample.choice");
    draw_frames(window, cx, 3);
    assert!(visual.debug_bounds("MENU_ITEM-first").is_some());
    visual.simulate_keystrokes("down down up enter");
    draw_frames(window, cx, 3);

    assert!(
        visual.debug_bounds("MENU_ITEM-first").is_none(),
        "Enter closes the dropdown menu after choosing an option"
    );
    assert!(visual.update(|window, cx| yard_shell::has_modal(window, cx)));
    assert_eq!(
        read_json(&settings_path)["choice_sample"]["choice"],
        json!("second")
    );
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<ChoiceSection>(cx).choice,
            ThreeChoices::Second
        );
    });

    // Reopen from `second` (1): Up -> `first` (0), Enter selects `first`.
    click(&mut visual, "field:choice_sample.choice");
    draw_frames(window, cx, 3);
    visual.simulate_keystrokes("up enter");
    draw_frames(window, cx, 3);

    assert!(visual.debug_bounds("MENU_ITEM-first").is_none());
    assert_eq!(
        read_json(&settings_path)["choice_sample"]["choice"],
        json!("first")
    );
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<ChoiceSection>(cx).choice,
            ThreeChoices::First
        );
    });
}

#[gpui::test]
fn submitting_empty_nullable_text_field_clears_empty_string_override(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(
        &settings_path,
        r#"{"nullable_sample": {"nickname": "", "keep": true}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "nullable_sample").unwrap();
        sample.register_editable_settings::<NullableSection>(cx);
        sample.register(cx, "nullable_sample.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "nullable_sample.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:nullable_sample");
    draw_frames(window, cx, 1);

    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<NullableSection>(cx)
                .nickname
                .as_deref(),
            Some("")
        );
    });
    assert!(
        visual
            .debug_bounds("reset:nullable_sample.nickname")
            .is_some(),
        "empty-string override on nullable text field starts modified"
    );

    click(&mut visual, "field:nullable_sample.nickname");
    draw_frames(window, cx, 1);
    visual.simulate_keystrokes("enter");
    draw_frames(window, cx, 2);

    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<NullableSection>(cx),
            &NullableSection {
                nickname: None,
                keep: true,
            }
        );
    });
    assert_eq!(
        read_json(&settings_path),
        json!({"nullable_sample": {"keep": true}})
    );
    assert!(
        visual
            .debug_bounds("reset:nullable_sample.nickname")
            .is_none()
    );
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
enum CollidingDefaultChoice {
    #[default]
    #[serde(rename = "Use default")]
    UseDefault,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default)]
struct CollidingNullableChoiceSection {
    mode: Option<CollidingDefaultChoice>,
    keep: bool,
}

impl Settings for CollidingNullableChoiceSection {
    const KEY: &'static str = "colliding_nullable_choice";
}

#[gpui::test]
fn nullable_choice_with_use_default_variant_names_reset_option_use_default_setting(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let settings_path = temp.path().join("settings.json");
    fs::write(
        &settings_path,
        r#"{"colliding_nullable_choice": {"keep": true}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings_path.clone());
        yard_ui::init(cx);
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);

        let editor = Extension::new(cx, "settings_editor").unwrap();
        settings_editor::init(cx, &editor);

        let sample = Extension::new(cx, "colliding_nullable_choice").unwrap();
        sample.register_editable_settings::<CollidingNullableChoiceSection>(cx);
        sample.register(cx, "colliding_nullable_choice.card", &[], |_, _, _, cx| {
            cx.new(|_| Card).into()
        });
        sample.place_root(cx, "root", "colliding_nullable_choice.card");
    });

    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), yard_shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(800.)));
    draw_frames(window, cx, 1);

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.dispatch_action(settings_editor::Open);
    draw_frames(window, cx, 2);
    click(&mut visual, "section:colliding_nullable_choice");
    draw_frames(window, cx, 1);

    assert!(
        visual
            .debug_bounds("reset:colliding_nullable_choice.mode")
            .is_none()
    );

    click(&mut visual, "field:colliding_nullable_choice.mode");
    draw_frames(window, cx, 3);

    assert!(
        visual.debug_bounds("MENU_ITEM-Use default").is_some(),
        "enum variant renamed 'Use default' is shown"
    );
    assert!(
        visual.debug_bounds("MENU_ITEM-custom").is_some(),
        "enum variant 'custom' is shown"
    );
    assert!(
        visual
            .debug_bounds("MENU_ITEM-Use default setting")
            .is_some(),
        "reset option is named 'Use default setting' when a choice is already named 'Use default'"
    );

    // Choosing the `"Use default"` enum variant saves `"Use default"`, distinct from resetting.
    click(&mut visual, "MENU_ITEM-Use default");
    draw_frames(window, cx, 3);

    assert_eq!(
        read_json(&settings_path),
        json!({"colliding_nullable_choice": {"keep": true, "mode": "Use default"}})
    );
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<CollidingNullableChoiceSection>(cx).mode,
            Some(CollidingDefaultChoice::UseDefault)
        );
    });
    assert!(
        visual
            .debug_bounds("reset:colliding_nullable_choice.mode")
            .is_some(),
        "choosing the 'Use default' variant marks the nullable field modified"
    );

    // Choosing `"Use default setting"` resets `mode` back to `None`.
    click(&mut visual, "field:colliding_nullable_choice.mode");
    draw_frames(window, cx, 3);
    click(&mut visual, "MENU_ITEM-Use default setting");
    draw_frames(window, cx, 3);

    assert_eq!(
        read_json(&settings_path),
        json!({"colliding_nullable_choice": {"keep": true}})
    );
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<CollidingNullableChoiceSection>(cx).mode,
            None
        );
    });
    assert!(
        visual
            .debug_bounds("reset:colliding_nullable_choice.mode")
            .is_none()
    );
}
