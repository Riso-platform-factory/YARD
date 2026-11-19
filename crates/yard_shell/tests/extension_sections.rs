use gpui::TestAppContext;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
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
            "yard-extension-sections-{}-{}-{nanos:x}",
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

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Tour {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for Tour {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for Tour {
    const KEY: &'static str = "tour";
}

#[derive(serde::Deserialize, PartialEq, Debug)]
#[serde(default)]
struct Theme {
    scheme: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            scheme: "light".into(),
        }
    }
}

impl Settings for Theme {
    const KEY: &'static str = "theme";
}

#[derive(serde::Deserialize, Default)]
struct OtherTheme {}

impl Settings for OtherTheme {
    const KEY: &'static str = Theme::KEY;
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct Counted {
    call: u32,
}

impl Default for Counted {
    fn default() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Self {
            call: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl Settings for Counted {
    const KEY: &'static str = "tour";
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
fn the_extension_named_section_gets_the_files_values(cx: &mut TestAppContext) {
    let home = TempDir::new();
    let path = settings_file(
        &home,
        r#"{"tour": {"greeting": "hi", "size": 20, "enabled": false}}"#,
    );
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let tour = yard_shell::Extension::new(cx, "tour").unwrap();
        tour.register_settings::<Tour>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<Tour>(cx),
            &Tour {
                greeting: "hi".into(),
                size: 20,
                enabled: false,
            }
        );
    });
}

#[gpui::test]
fn the_extension_named_section_gets_its_defaults_without_the_files_section(
    cx: &mut TestAppContext,
) {
    let home = TempDir::new();
    let path = settings_file(&home, r#"{"undeclared": {"ignored": true}}"#);
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let tour = yard_shell::Extension::new(cx, "tour").unwrap();
        tour.register_settings::<Tour>(cx);
    });
    cx.read(|cx| {
        assert_eq!(yard_settings::get::<Tour>(cx), &Tour::default());
    });
}

#[gpui::test]
fn a_refused_key_leaves_the_section_free_for_its_extension(cx: &mut TestAppContext) {
    let home = TempDir::new();
    let path = settings_file(
        &home,
        r#"{"tour": {"greeting": "hi", "size": 20, "enabled": false}}"#,
    );
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let guest = yard_shell::Extension::new(cx, "guest").unwrap();
        guest.register_settings::<Tour>(cx);
        assert_unregistered::<Tour>(cx);
        let tour = yard_shell::Extension::new(cx, "tour").unwrap();
        tour.register_settings::<Tour>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<Tour>(cx),
            &Tour {
                greeting: "hi".into(),
                size: 20,
                enabled: false,
            }
        );
    });
}

#[gpui::test]
fn an_extension_cannot_take_a_section_an_engine_registered(cx: &mut TestAppContext) {
    let home = TempDir::new();
    let path = settings_file(&home, r#"{"theme": {"scheme": "dark"}}"#);
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        yard_settings::register::<Theme>(cx);
        let other = yard_shell::Extension::new(cx, "other").unwrap();
        other.register_settings::<OtherTheme>(cx);
        assert_unregistered::<OtherTheme>(cx);
    });
    cx.read(|cx| {
        assert_eq!(
            yard_settings::get::<Theme>(cx),
            &Theme {
                scheme: "dark".into(),
            }
        );
    });
}

#[gpui::test]
fn a_second_registration_of_the_same_section_is_refused(cx: &mut TestAppContext) {
    let home = TempDir::new();
    let path = settings_file(&home, "{}");
    cx.update(|cx| {
        yard_settings::init(cx);
        yard_settings::load(cx, path);
        let tour = yard_shell::Extension::new(cx, "tour").unwrap();
        tour.register_settings::<Counted>(cx);
        tour.register_settings::<Counted>(cx);
    });
    cx.read(|cx| {
        // The engine refused the second registration: the first value stays.
        assert_eq!(yard_settings::get::<Counted>(cx).call, 1);
    });
}
