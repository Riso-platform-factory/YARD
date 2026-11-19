use gpui::{Hsla, Rgba, TestAppContext};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_ui::load_user_themes;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-ui-user-themes-{}-{}-{nanos:x}",
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

// Small theme families in Zed's format, as an extension ships them: a `name`,
// an `author`, and `themes`, each theme with a `name`, an `appearance`
// (`light` or `dark`), and a `style` that names its colors with dots.
const AYU: &str = r##"{
    "name": "Ayu",
    "author": "yard's tests",
    "themes": [
        {"name": "Ayu Dark", "appearance": "dark", "style": {"background": "#0d1016ff"}},
        {"name": "Ayu Light", "appearance": "light", "style": {"background": "#f8f9fbff"}},
        {"name": "Ayu Mirage", "appearance": "dark", "style": {"background": "#1f2430ff"}}
    ]
}"##;

const ONE_DARK: &str = r##"{
    "name": "User One",
    "author": "yard's tests",
    "themes": [
        {"name": "One Dark", "appearance": "dark", "style": {"background": "#112233ff"}}
    ]
}"##;

const SHARED_EARLY: &str = r##"{
    "name": "Early",
    "author": "yard's tests",
    "themes": [
        {"name": "Shared Dark", "appearance": "dark", "style": {"background": "#111111ff"}}
    ]
}"##;

const SHARED_LATE: &str = r##"{
    "name": "Late",
    "author": "yard's tests",
    "themes": [
        {"name": "Shared Dark", "appearance": "dark", "style": {"background": "#222222ff"}}
    ]
}"##;

// A `themes` folder inside `temp`, created and holding the named files.
fn theme_dir(temp: &TempDir, files: &[(&str, &str)]) -> PathBuf {
    let dir = temp.path().join("themes");
    fs::create_dir_all(&dir).unwrap();
    for (name, content) in files {
        fs::write(dir.join(name), content).unwrap();
    }
    dir
}

fn registered(cx: &gpui::App, name: &str) -> bool {
    theme::ThemeRegistry::global(cx).get(name).is_ok()
}

fn hex(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.r * 255.).round() as u8,
        (rgba.g * 255.).round() as u8,
        (rgba.b * 255.).round() as u8
    )
}

#[gpui::test]
fn a_family_of_three_themes_registers_three(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let dir = theme_dir(&temp, &[("ayu.json", AYU)]);
    let count = cx.update(|cx| {
        yard_ui::init(cx);
        load_user_themes(cx, &dir)
    });
    assert_eq!(count, 3);
    cx.read(|cx| {
        assert!(registered(cx, "Ayu Dark"));
        assert!(registered(cx, "Ayu Light"));
        assert!(registered(cx, "Ayu Mirage"));
    });
}

#[gpui::test]
fn a_file_that_cannot_be_read_leaves_the_other_files_to_load(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    // `a-broken.json` sorts before `z-ayu.json`, so the load goes on after the
    // file it cannot read.
    let dir = theme_dir(
        &temp,
        &[
            ("a-broken.json", r#"{"name": "Broken", "themes": ["#),
            ("z-ayu.json", AYU),
        ],
    );
    let count = cx.update(|cx| {
        yard_ui::init(cx);
        load_user_themes(cx, &dir)
    });
    assert_eq!(count, 3);
    cx.read(|cx| {
        assert!(registered(cx, "Ayu Dark"));
        assert!(registered(cx, "Ayu Light"));
        assert!(registered(cx, "Ayu Mirage"));
    });
}

#[gpui::test]
fn a_file_whose_name_does_not_end_in_json_is_skipped(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    // The file holds a valid family: only its name keeps it out.
    let dir = theme_dir(&temp, &[("ayu.txt", AYU)]);
    let count = cx.update(|cx| {
        yard_ui::init(cx);
        load_user_themes(cx, &dir)
    });
    assert_eq!(count, 0);
    cx.read(|cx| assert!(!registered(cx, "Ayu Dark")));
}

#[gpui::test]
fn a_user_theme_named_after_a_bundled_theme_replaces_it(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let dir = theme_dir(&temp, &[("one.json", ONE_DARK)]);
    let settings = temp.path().join("settings.json");
    fs::write(&settings, r#"{"theme": {"mode": "dark"}}"#).unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings.clone());
        yard_ui::init(cx);
    });
    // The bundled One Dark is the active theme before the user's files load.
    cx.read(|cx| {
        assert_eq!(yard_ui::active_theme(cx), "One Dark");
        assert_eq!(hex(yard_ui::palette(cx).background), "#3b414d");
    });
    cx.update(|cx| {
        assert_eq!(load_user_themes(cx, &dir), 1);
        yard_ui::apply_theme_setting(cx);
    });
    // The chosen `One Dark` is now the user file's: the background is the one
    // the file names, not the bundled `#3b414d`.
    cx.read(|cx| {
        assert_eq!(yard_ui::active_theme(cx), "One Dark");
        assert_eq!(hex(yard_ui::palette(cx).background), "#112233");
    });
}

#[gpui::test]
fn a_theme_named_by_two_files_keeps_the_one_from_the_later_file(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    // The files are read in the order of their names, so `z-late.json`
    // replaces what `a-early.json` registered under `Shared Dark`.
    let dir = theme_dir(
        &temp,
        &[("a-early.json", SHARED_EARLY), ("z-late.json", SHARED_LATE)],
    );
    cx.update(|cx| {
        yard_ui::init(cx);
        load_user_themes(cx, &dir);
    });
    let background = cx.read(|cx| {
        theme::ThemeRegistry::global(cx)
            .get("Shared Dark")
            .map(|theme| hex(theme.styles.colors.background))
    });
    assert_eq!(background.as_deref().ok(), Some("#222222"));
}

#[gpui::test]
fn a_folder_that_does_not_exist_registers_nothing(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let dir = temp.path().join("absent");
    assert!(!dir.exists());
    let count = cx.update(|cx| {
        yard_ui::init(cx);
        load_user_themes(cx, &dir)
    });
    assert_eq!(count, 0);
    cx.read(|cx| {
        assert!(!registered(cx, "Ayu Dark"));
        assert!(registered(cx, "One Dark"), "the bundled themes stay");
    });
}

#[gpui::test]
fn a_settings_file_naming_a_user_theme_makes_it_active(cx: &mut TestAppContext) {
    let temp = TempDir::new();
    let dir = theme_dir(&temp, &[("ayu.json", AYU)]);
    let settings = temp.path().join("settings.json");
    fs::write(
        &settings,
        r#"{"theme": {"mode": "dark", "dark": "Ayu Dark"}}"#,
    )
    .unwrap();

    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, settings.clone());
        yard_ui::init(cx);
        // After `yard_ui::init` and before the theme setting is applied, as
        // `app-init` does it, so the user theme is active from the first frame.
        assert_eq!(load_user_themes(cx, &dir), 3);
        yard_ui::apply_theme_setting(cx);
    });
    cx.read(|cx| {
        assert_eq!(yard_ui::active_theme(cx), "Ayu Dark");
        assert_eq!(hex(yard_ui::palette(cx).background), "#0d1016");
    });
}
