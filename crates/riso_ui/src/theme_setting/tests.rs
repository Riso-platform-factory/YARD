use super::*;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-ui-theme-setting-{}-{}-{nanos:x}",
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

fn settings_file(temp: &TempDir, content: &str) -> std::path::PathBuf {
    let path = temp.path().join("settings.json");
    std::fs::write(&path, content).unwrap();
    path
}

// The startup's order: the store, the file, the UI, then the theme setting.
fn startup(cx: &mut gpui::App, settings: std::path::PathBuf) {
    riso_settings::init(cx);
    riso_settings::load(cx, settings);
    crate::init(cx);
    apply_theme_setting(cx);
}

fn rgb(color: gpui::Hsla) -> (u8, u8, u8) {
    let rgba = gpui::Rgba::from(color);
    (
        (rgba.r * 255.).round() as u8,
        (rgba.g * 255.).round() as u8,
        (rgba.b * 255.).round() as u8,
    )
}

#[gpui::test]
fn without_a_settings_file_one_dark_is_active(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert!(!path.exists());
}

#[gpui::test]
fn without_the_theme_section_one_dark_is_active(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, "{}");
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
}

#[gpui::test]
fn light_mode_activates_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
}

#[gpui::test]
fn dark_mode_activates_the_theme_named_by_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dark", "dark": "One Light"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
}

#[gpui::test]
fn an_unregistered_theme_name_keeps_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(
        &temp,
        r#"{"theme": {"mode": "light", "light": "Solarized"}}"#,
    );
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
}

#[gpui::test]
fn a_mode_of_another_value_keeps_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dim"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
}

#[gpui::test]
fn a_theme_section_that_is_not_an_object_keeps_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": "One Dark"}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
}

#[gpui::test]
fn the_palettes_background_differs_between_one_dark_and_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let settings = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| {
        riso_settings::init(cx);
        riso_settings::load(cx, settings);
        crate::init(cx);
    });
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let dark = cx.read(|cx| rgb(crate::palette(cx).background));
    cx.update(apply_theme_setting);
    cx.read(|cx| {
        assert_eq!(active_theme(cx), "One Light");
        assert_ne!(dark, rgb(crate::palette(cx).background));
    });
}
