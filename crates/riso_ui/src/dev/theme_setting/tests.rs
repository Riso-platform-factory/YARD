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

// The startup's order: the store, the file, the watch, the UI, then the theme setting.
fn startup(cx: &mut gpui::App, settings: std::path::PathBuf) {
    riso_settings::init(cx);
    riso_settings::load(cx, settings);
    riso_settings::watch(cx);
    crate::dev::init(cx);
    apply_theme_setting(cx);
}

// The same startup while the OS's appearance is dark: `init` records the test
// platform's light, so dark is recorded again before the setting applies.
fn startup_dark(cx: &mut gpui::App, settings: std::path::PathBuf) {
    riso_settings::init(cx);
    riso_settings::load(cx, settings);
    riso_settings::watch(cx);
    crate::dev::init(cx);
    record(cx, theme::Appearance::Dark);
    apply_theme_setting(cx);
}

// The appearance the OS reports, recorded the way `init` and a window's report record it.
fn record(cx: &mut gpui::App, appearance: theme::Appearance) {
    theme::SystemAppearance::global_mut(cx).0 = appearance;
}

fn rgb(color: gpui::Hsla) -> (u8, u8, u8) {
    let rgba = gpui::Rgba::from(color);
    (
        (rgba.r * 255.).round() as u8,
        (rgba.g * 255.).round() as u8,
        (rgba.b * 255.).round() as u8,
    )
}

// One second of the test clock runs the watch's next check.
fn tick(cx: &gpui::TestAppContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
}

// A switch sets the theme global again; `unchanged` does not. The returned
// subscription keeps the count alive.
fn theme_switches(cx: &mut gpui::App) -> (std::rc::Rc<std::cell::Cell<usize>>, gpui::Subscription) {
    let switches = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let counted = switches.clone();
    let subscription = cx.observe_global::<theme::GlobalTheme>(move |_| {
        counted.set(counted.get() + 1);
    });
    (switches, subscription)
}

// A window the theme setting follows: opening it reports the OS's appearance.
fn window(cx: &mut gpui::TestAppContext) {
    cx.add_window(|window, cx| {
        follow_appearance(window, cx);
        gpui::EmptyView
    });
    cx.run_until_parked();
}

// Without a section the mode is now `system`: the test platform's light
// appearance gives One Light.
#[gpui::test]
fn without_a_settings_file_one_light_is_active(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert!(!path.exists());
}

#[gpui::test]
fn without_the_theme_section_one_light_is_active(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, "{}");
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
}

// Without a section, system follows the OS: a dark startup applies One Dark.
#[gpui::test]
fn without_a_section_a_dark_appearance_activates_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    cx.update(|cx| startup_dark(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert!(!path.exists());
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

// An unreadable section falls back to the defaults, whose mode is system.
#[gpui::test]
fn a_mode_of_another_value_activates_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dim"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
}

#[gpui::test]
fn a_theme_section_that_is_not_an_object_activates_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": "One Dark"}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
}

#[gpui::test]
fn the_palettes_background_differs_between_one_dark_and_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let settings = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| {
        riso_settings::init(cx);
        riso_settings::load(cx, settings);
        crate::dev::init(cx);
    });
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let dark = cx.read(|cx| rgb(crate::palette(cx).background));
    cx.update(apply_theme_setting);
    cx.read(|cx| {
        assert_eq!(active_theme(cx), "One Light");
        assert_ne!(dark, rgb(crate::palette(cx).background));
    });
}

// Startup with no file leaves One Light; an edit to dark switches.
#[gpui::test]
fn an_edit_from_no_file_to_dark_switches_to_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    let (switches, _subscription) = cx.update(theme_switches);

    std::fs::write(&path, r#"{"theme": {"mode": "dark"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert_eq!(switches.get(), 1);
}

// Deleting the file brings the defaults back: system on a light platform.
#[gpui::test]
fn deleting_the_file_switches_to_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dark"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let (switches, _subscription) = cx.update(theme_switches);

    std::fs::remove_file(&path).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 1);
}

#[gpui::test]
fn an_edit_of_dark_while_mode_is_light_does_not_switch(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    let (switches, _subscription) = cx.update(theme_switches);

    // The section changed, but the theme `mode` chooses is the one already active.
    std::fs::write(
        &path,
        r#"{"theme": {"mode": "light", "dark": "One Light"}}"#,
    )
    .unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 0);
}

#[gpui::test]
fn an_edit_to_an_unregistered_name_keeps_the_active_theme(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    let (switches, _subscription) = cx.update(theme_switches);

    std::fs::write(
        &path,
        r#"{"theme": {"mode": "light", "light": "Solarized"}}"#,
    )
    .unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 0);
}

#[gpui::test]
fn a_broken_edit_keeps_the_active_theme(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "light"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    let (switches, _subscription) = cx.update(theme_switches);

    // The section's new content cannot be read: the last good value stays.
    std::fs::write(&path, r#"{"theme": {"mode": "dim"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 0);
}

// A window reports the OS's appearance. While the mode is system the theme of
// the new appearance becomes active.
#[gpui::test]
fn in_system_mode_a_reported_light_appearance_activates_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "system"}}"#);
    cx.update(|cx| startup_dark(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let (switches, _subscription) = cx.update(theme_switches);

    window(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 1);
}

// While the mode is dark a change of appearance is recorded and the theme stays.
#[gpui::test]
fn in_dark_mode_a_reported_light_appearance_keeps_one_dark(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dark"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let (switches, _subscription) = cx.update(theme_switches);

    // The recorded appearance is dark, so the window's light report is a change.
    cx.update(|cx| record(cx, theme::Appearance::Dark));
    window(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert_eq!(switches.get(), 0);
}

// The section's mode changes to system: the theme of the current appearance is
// applied — dark here, so the theme stays.
#[gpui::test]
fn an_edit_to_system_applies_the_theme_of_the_current_appearance(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dark"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let (switches, _subscription) = cx.update(theme_switches);

    // The OS's appearance went dark while the mode was dark: it is recorded.
    cx.update(|cx| record(cx, theme::Appearance::Dark));

    std::fs::write(&path, r#"{"theme": {"mode": "system"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    assert_eq!(switches.get(), 0);
}

// The same edit while the current appearance is light switches to One Light.
#[gpui::test]
fn an_edit_to_system_switches_to_one_light(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "dark"}}"#);
    cx.update(|cx| startup(cx, path.clone()));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Dark"));
    let (switches, _subscription) = cx.update(theme_switches);

    std::fs::write(&path, r#"{"theme": {"mode": "system"}}"#).unwrap();
    tick(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 1);
}

// The same appearance reported twice does nothing.
#[gpui::test]
fn the_same_appearance_reported_twice_changes_nothing(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"mode": "system"}}"#);
    cx.update(|cx| startup(cx, path));
    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    let (switches, _subscription) = cx.update(theme_switches);

    window(cx);
    window(cx);

    cx.read(|cx| assert_eq!(active_theme(cx), "One Light"));
    assert_eq!(switches.get(), 0);
}

// ----- ui-font -----

// A test app backed by the machine's text system, so that installed families —
// Menlo among them, and IBM Plex Sans once the bundled fonts register — are
// known. The default test text system knows no font at all.
#[cfg(target_os = "macos")]
fn fonts_app(cx: &gpui::TestAppContext) -> gpui::TestAppContext {
    gpui::TestAppContext::build_with_text_system(
        cx.dispatcher.clone(),
        None,
        std::sync::Arc::new(gpui_apple::AppleTextSystem::new()),
    )
}

// The font `ui_font` and `ui_font_size` give is the one the settings provider
// gives Zed's components.
#[cfg(target_os = "macos")]
fn assert_ui_font(cx: &gpui::App, family: &str, size: f32) {
    let font = crate::dev::ui_font(cx);
    let size = gpui::px(size);
    assert_eq!(font.family, family);
    assert_eq!(crate::dev::ui_font_size(cx), size);
    let provider = theme::theme_settings(cx);
    assert_eq!(provider.ui_font(cx).family, font.family);
    assert_eq!(provider.ui_font_size(cx), size);
}

// Without values the bundled font applies at 16px.
#[cfg(target_os = "macos")]
#[gpui::test]
fn without_values_the_ui_font_is_ibm_plex_sans_at_16(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path.clone()));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 16.));
    assert!(!path.exists());
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn a_ui_font_size_in_the_section_applies(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 20}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 20.));
}

// A size below 6 clamps to 6, as Zed does.
#[cfg(target_os = "macos")]
#[gpui::test]
fn a_ui_font_size_below_6_is_clamped_to_6(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 2}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 6.));
}

// A size above 100 clamps to 100.
#[cfg(target_os = "macos")]
#[gpui::test]
fn a_ui_font_size_above_100_is_clamped_to_100(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 200}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 100.));
}

// The size is a number of pixels, with or without decimals.
#[cfg(target_os = "macos")]
#[gpui::test]
fn a_ui_font_size_with_decimals_applies(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 13.5}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 13.5));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn an_installed_ui_font_family_applies(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_family": "Menlo"}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "Menlo", 16.));
}

// The default family applies instead, and the size still applies.
#[cfg(target_os = "macos")]
#[gpui::test]
fn a_ui_font_family_that_is_not_installed_keeps_the_default(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(
        &temp,
        r#"{"theme": {"ui_font_family": "Nope Sans", "ui_font_size": 20}}"#,
    );
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 20.));
}

// A change of the size while running applies at once, as a change of theme does.
#[cfg(target_os = "macos")]
#[gpui::test]
fn an_edit_of_the_ui_font_size_applies(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 16}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path.clone()));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 16.));

    std::fs::write(&path, r#"{"theme": {"ui_font_size": 24}}"#).unwrap();
    tick(&app);

    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 24.));
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn an_edit_of_the_ui_font_family_applies(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = temp.path().join("settings.json");
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path.clone()));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 16.));

    std::fs::write(&path, r#"{"theme": {"ui_font_family": "Menlo"}}"#).unwrap();
    tick(&app);

    app.read(|cx| assert_ui_font(cx, "Menlo", 16.));
}

// Only the theme changed: the font is as it was.
#[cfg(target_os = "macos")]
#[gpui::test]
fn an_edit_of_only_the_theme_leaves_the_ui_font_as_it_was(cx: &mut gpui::TestAppContext) {
    let temp = TempDir::new();
    let path = settings_file(&temp, r#"{"theme": {"ui_font_size": 20}}"#);
    let app = fonts_app(cx);
    app.update(|cx| startup(cx, path.clone()));
    app.read(|cx| assert_ui_font(cx, "IBM Plex Sans", 20.));

    std::fs::write(&path, r#"{"theme": {"mode": "dark", "ui_font_size": 20}}"#).unwrap();
    tick(&app);

    app.read(|cx| {
        assert_eq!(active_theme(cx), "One Dark");
        assert_ui_font(cx, "IBM Plex Sans", 20.);
    });
}
