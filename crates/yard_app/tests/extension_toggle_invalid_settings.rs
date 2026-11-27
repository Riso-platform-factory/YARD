use gpui::{Global, TestAppContext};
use std::{
    cell::RefCell,
    fs,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use yard_app::{Application, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() == log::Level::Warn
            && matches!(
                metadata.target(),
                "yard_app::startup" | "yard_settings::sections"
            )
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            WARNINGS.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

#[derive(Default)]
struct Started(Vec<&'static str>);

impl Global for Started {}

#[gpui::test]
fn invalid_extension_settings_warn_once_and_preserve_valid_entries(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "yard-toggle-invalid-{}-{unique}",
        std::process::id()
    ));
    let paths = Paths::under(&home);
    let settings = paths.settings_file();
    fs::create_dir_all(settings.parent().unwrap()).unwrap();

    for (input, started, warning) in [
        (
            r#"{"extensions":{"command_palette":false,"x":"no"}}"#,
            &["other"][..],
            r#"extension "x" in the extensions setting is not true or false"#,
        ),
        (
            r#"{"extensions":3}"#,
            &["command_palette", "other"][..],
            "section extensions cannot be read: the section is not a JSON object",
        ),
    ] {
        WARNINGS.lock().unwrap().clear();
        fs::write(&settings, input).unwrap();
        let app = cx.new_app();
        app.update(|cx| {
            yard_app::init(
                cx,
                Application {
                    product: Product {
                        name: "Toggle Test",
                        version: "0.1.0",
                        identifier: "test.yard.toggle",
                    },
                    url_scheme: "toggletest",
                    update_url: "",
                    window_icon: None,
                },
                &paths,
                &[
                    ("command_palette", |cx, _| {
                        cx.default_global::<Started>().0.push("command_palette")
                    }),
                    ("other", |cx, _| {
                        cx.default_global::<Started>().0.push("other")
                    }),
                ],
                |_| Vec::new(),
                &RefCell::new(OpenUrls::default()),
            );
            assert_eq!(cx.global::<Started>().0, started);
        });
        app.quit();
        assert_eq!(*WARNINGS.lock().unwrap(), [warning]);
    }

    fs::remove_dir_all(home).unwrap();
}
