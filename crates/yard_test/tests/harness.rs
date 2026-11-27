use gpui::{App, AppContext as _, Global, Menu, SystemNotification, TestAppContext};
use serde_json::{Value, json};
use std::{
    any::Any,
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    time::Duration,
};
use yard_app::Application;
use yard_paths::{Origin, Paths};
use yard_product::Product;
use yard_settings::Settings;
use yard_shell::Extension;
use yard_test::TestApp;

#[derive(Default)]
struct Probe {
    init_order: Vec<&'static str>,
    initial_settings: Option<NotesSettings>,
    observed_titles: Vec<String>,
    opened_urls: Vec<String>,
}

impl Global for Probe {}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
struct NotesSettings {
    title: String,
    count: u32,
}

impl Default for NotesSettings {
    fn default() -> Self {
        Self {
            title: "Default".to_owned(),
            count: 0,
        }
    }
}

impl Settings for NotesSettings {
    const KEY: &'static str = "notes";
}

fn first_extension(cx: &mut App, _extension: &Extension) {
    cx.default_global::<Probe>().init_order.push("first");
}

fn second_extension(cx: &mut App, _extension: &Extension) {
    cx.default_global::<Probe>().init_order.push("second");
}

fn notes_extension(cx: &mut App, extension: &Extension) {
    extension.register_settings::<NotesSettings>(cx);
    let current = yard_settings::get::<NotesSettings>(cx).clone();
    cx.default_global::<Probe>().initial_settings = Some(current);
    yard_settings::observe::<NotesSettings>(cx, |cx| {
        let title = yard_settings::get::<NotesSettings>(cx).title.clone();
        cx.default_global::<Probe>().observed_titles.push(title);
    })
    .detach();
}

fn url_probe_extension(cx: &mut App, extension: &Extension) {
    extension.on_open_url(cx, |url, cx| {
        cx.default_global::<Probe>()
            .opened_urls
            .push(url.to_owned());
    });
}

fn notify_extension(cx: &mut App, extension: &Extension) {
    extension.show_notification(
        SystemNotification {
            tag: "ready".into(),
            title: "Ready".into(),
            body: "Extension booted".into(),
            actions: Vec::new(),
        },
        cx,
    );
}

fn panic_text(payload: &Box<dyn Any + Send>) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

struct RemoveDirOnDrop(PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn product_menus(product: &Product) -> Vec<Menu> {
    vec![
        Menu::new(product.name),
        Menu::new(product.version),
        Menu::new(product.identifier),
    ]
}

fn home_of(paths: &Paths) -> PathBuf {
    paths
        .config_dir()
        .parent()
        .expect("config_dir sits under the temporary home")
        .to_path_buf()
}

#[gpui::test]
fn boot_runs_extensions_in_order_and_opens_one_window(cx: &mut TestAppContext) {
    let app = TestApp::builder()
        .extension("first", first_extension)
        .extension("second", second_extension)
        .boot(cx);

    let init_order = cx.read(|cx| cx.global::<Probe>().init_order.clone());
    assert_eq!(
        init_order,
        vec!["first", "second"],
        "boot runs each extension's init once, in the order given"
    );

    let windows = cx.windows();
    assert_eq!(windows.len(), 1, "boot opens exactly one window");
    assert_eq!(
        windows[0],
        app.window().into(),
        "app.window() returns the single window opened by boot"
    );

    let mut visual = app.visual(cx);
    assert_eq!(
        visual.update(|window, _| window.window_handle()),
        app.window().into(),
        "app.visual(cx) targets the window returned by app.window()"
    );
    assert!(
        visual.debug_bounds("WINDOW-TITLE-Test App").is_some(),
        "boot draws the window with the default title"
    );

    cx.update(|cx| yard_shell::set_window_title(cx, "Redrawn Title"));
    app.draw(cx);
    assert!(
        visual.debug_bounds("WINDOW-TITLE-Redrawn Title").is_some(),
        "app.draw(cx) redraws the window"
    );
}

#[gpui::test]
fn extension_sees_initial_settings_at_init(cx: &mut TestAppContext) {
    let _app = TestApp::builder()
        .extension("notes", notes_extension)
        .settings(json!({ "notes": { "title": "Inbox", "count": 3 } }))
        .boot(cx);

    let seen_at_init = cx.read(|cx| cx.global::<Probe>().initial_settings.clone());
    assert_eq!(
        seen_at_init,
        Some(NotesSettings {
            title: "Inbox".to_owned(),
            count: 3,
        }),
        "an extension sees values from .settings(...) during init"
    );

    let current = cx.read(|cx| yard_settings::get::<NotesSettings>(cx).clone());
    assert_eq!(
        current,
        NotesSettings {
            title: "Inbox".to_owned(),
            count: 3,
        }
    );
}

#[gpui::test]
fn write_settings_reaches_observer_before_returning(cx: &mut TestAppContext) {
    let app = TestApp::builder()
        .extension("notes", notes_extension)
        .settings(json!({ "notes": { "title": "Inbox", "count": 1 } }))
        .boot(cx);

    assert!(
        cx.read(|cx| cx.global::<Probe>().observed_titles.is_empty()),
        "observer has not fired before any settings change"
    );

    app.write_settings(cx, json!({ "notes": { "title": "Later", "count": 2 } }));

    // No run_until_parked or clock advance here: the observer must have run
    // before write_settings returned.
    let observed = cx.read(|cx| cx.global::<Probe>().observed_titles.clone());
    assert_eq!(
        observed,
        vec!["Later"],
        "write_settings notifies the observer before returning"
    );

    let current = cx.read(|cx| yard_settings::get::<NotesSettings>(cx).clone());
    assert_eq!(
        current,
        NotesSettings {
            title: "Later".to_owned(),
            count: 2,
        }
    );

    let saved: Value = serde_json::from_slice(
        &fs::read(app.paths().settings_file()).expect("settings.json exists on disk"),
    )
    .expect("settings.json is valid JSON");
    assert_eq!(saved["notes"]["title"], json!("Later"));
    assert_eq!(saved["notes"]["count"], json!(2));
}

#[gpui::test]
fn paths_use_distinct_temporary_homes_removed_on_drop(cx: &mut TestAppContext) {
    let mut first_cx = cx.new_app();
    let mut second_cx = cx.new_app();

    let mut first = Some(TestApp::builder().boot(&mut first_cx));
    let mut second = Some(TestApp::builder().boot(&mut second_cx));

    let first_home = home_of(first.as_ref().unwrap().paths());
    let second_home = home_of(second.as_ref().unwrap().paths());

    assert_eq!(first.as_ref().unwrap().paths().origin(), Origin::Given);
    assert_eq!(second.as_ref().unwrap().paths().origin(), Origin::Given);
    assert_ne!(
        first_home, second_home,
        "two TestApps must have different homes"
    );

    let temp_root = fs::canonicalize(std::env::temp_dir()).expect("temp_dir exists");
    for home in [&first_home, &second_home] {
        let canonical =
            fs::canonicalize(home).expect("temporary home exists while TestApp is alive");
        assert!(
            canonical.starts_with(&temp_root),
            "home {} is not under temporary directory {}",
            canonical.display(),
            temp_root.display(),
        );
    }

    first = None;
    assert!(
        !first_home.exists(),
        "dropping TestApp removes its temporary home {}",
        first_home.display(),
    );
    assert!(
        second_home.exists(),
        "dropping the first TestApp leaves the second home {} intact",
        second_home.display(),
    );

    second = None;
    assert!(
        !second_home.exists(),
        "dropping the second TestApp removes its temporary home {}",
        second_home.display(),
    );
    let _ = (first, second);
}

#[gpui::test]
fn default_application_is_test_app(cx: &mut TestAppContext) {
    let app = TestApp::builder()
        .extension("probe", url_probe_extension)
        .menus(product_menus)
        .boot(cx);

    let mut visual = app.visual(cx);
    assert!(
        visual.debug_bounds("WINDOW-TITLE-Test App").is_some(),
        "default window title is Test App"
    );

    let menu_names: Vec<String> = cx.read(|cx| {
        cx.get_menus()
            .expect("test platform keeps menus")
            .into_iter()
            .map(|menu| menu.name.to_string())
            .collect()
    });
    assert_eq!(
        menu_names,
        vec!["Test App", "0.0.0", "test.yard.app"],
        "default product is Test App 0.0.0 test.yard.app"
    );

    cx.update(|cx| {
        yard_shell::open_urls(
            cx,
            vec![
                "yard://probe/ignored".to_owned(),
                "yardtest://probe/accepted".to_owned(),
            ],
        );
    });
    assert_eq!(
        cx.read(|cx| cx.global::<Probe>().opened_urls.clone()),
        vec!["yardtest://probe/accepted"],
        "default URL scheme is yardtest"
    );
}

#[gpui::test]
fn custom_application_changes_product_title_and_scheme(cx: &mut TestAppContext) {
    let custom = Application {
        product: Product {
            name: "North Star",
            version: "2.3.4",
            identifier: "test.yard.northstar",
        },
        url_scheme: "northstar",
        update_url: "",
        window_icon: None,
    };

    let app = TestApp::builder()
        .application(custom)
        .extension("probe", url_probe_extension)
        .menus(product_menus)
        .boot(cx);

    let mut visual = app.visual(cx);
    assert!(
        visual.debug_bounds("WINDOW-TITLE-North Star").is_some(),
        "shell title bar shows the custom product name"
    );
    assert!(
        visual.debug_bounds("WINDOW-TITLE-Test App").is_none(),
        "default Test App title is replaced by the custom product name"
    );

    let menu_names: Vec<String> = cx.read(|cx| {
        cx.get_menus()
            .expect("test platform keeps menus")
            .into_iter()
            .map(|menu| menu.name.to_string())
            .collect()
    });
    assert_eq!(
        menu_names,
        vec!["North Star", "2.3.4", "test.yard.northstar"],
        "menus receive the custom Product"
    );

    cx.update(|cx| {
        yard_shell::open_urls(
            cx,
            vec![
                "yardtest://probe/ignored".to_owned(),
                "northstar://probe/accepted".to_owned(),
            ],
        );
    });
    assert_eq!(
        cx.read(|cx| cx.global::<Probe>().opened_urls.clone()),
        vec!["northstar://probe/accepted"],
        "shell uses the custom application's URL scheme"
    );
}

#[gpui::test]
fn boot_sets_app_identity_and_delivers_system_notifications(cx: &mut TestAppContext) {
    let mut default_cx = cx.new_app();
    let _default_app = TestApp::builder()
        .extension("alerts", notify_extension)
        .boot(&mut default_cx);

    assert_eq!(
        default_cx.app_identity(),
        Some(("test.yard.app".into(), "Test App".into())),
        "boot sets the default product's app identity"
    );
    assert_eq!(
        default_cx.shown_system_notifications(),
        vec![SystemNotification {
            tag: "alerts/ready".into(),
            title: "Ready".into(),
            body: "Extension booted".into(),
            actions: Vec::new(),
        }],
        "extension.show_notification reaches GPUI's test platform"
    );

    let mut custom_cx = cx.new_app();
    let custom = Application {
        product: Product {
            name: "North Star",
            version: "2.3.4",
            identifier: "test.yard.northstar",
        },
        url_scheme: "northstar",
        update_url: "",
        window_icon: None,
    };
    let _custom_app = TestApp::builder().application(custom).boot(&mut custom_cx);
    assert_eq!(
        custom_cx.app_identity(),
        Some(("test.yard.northstar".into(), "North Star".into())),
        "boot sets the custom product's app identity"
    );
}

#[gpui::test]
fn dropping_test_app_does_not_recreate_home_after_clock_advance_or_quit(cx: &mut TestAppContext) {
    let app = TestApp::builder().boot(cx);
    let home = home_of(app.paths());
    let _cleanup = RemoveDirOnDrop(home.clone());
    let window = app.window();

    cx.update_window(window.into(), |_, window, _| window.activate_window())
        .expect("activate test window");
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(900.), gpui::px(600.)));

    drop(app);
    assert!(
        !home.exists(),
        "dropping TestApp removes its temporary home {}",
        home.display(),
    );

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(
        !home.exists(),
        "advancing the clock and running until parked after dropping TestApp must not recreate {}",
        home.display(),
    );

    cx.quit();
    cx.run_until_parked();
    assert!(
        !home.exists(),
        "quitting the app after dropping TestApp must not recreate {}",
        home.display(),
    );
}

#[gpui::test]
fn panic_after_boot_leaves_no_home(cx: &mut TestAppContext) {
    let mut home = None;
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let app = TestApp::builder().boot(cx);
        home = Some(home_of(app.paths()));
        assert!(home.as_ref().unwrap().exists());
        panic!("simulated test panic after boot");
    }))
    .expect_err("panic after boot propagates");
    assert!(
        panic_text(&panicked).contains("simulated test panic after boot"),
        "expected simulated panic, got: {:?}",
        panic_text(&panicked)
    );

    let home = home.expect("boot recorded home path before panic");
    let _cleanup = RemoveDirOnDrop(home.clone());
    assert!(
        !home.exists(),
        "unwinding after boot removes the temporary home {}",
        home.display(),
    );

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(
        !home.exists(),
        "advancing the clock after a panic must not recreate {}",
        home.display(),
    );
}

#[gpui::test]
fn two_write_settings_in_a_row_each_reach_observer(cx: &mut TestAppContext) {
    let app = TestApp::builder()
        .extension("notes", notes_extension)
        .settings(json!({ "notes": { "title": "Initial", "count": 1 } }))
        .boot(cx);

    app.write_settings(cx, json!({ "notes": { "title": "First", "count": 2 } }));
    app.write_settings(cx, json!({ "notes": { "title": "Second", "count": 3 } }));

    assert_eq!(
        cx.read(|cx| cx.global::<Probe>().observed_titles.clone()),
        vec!["First", "Second"],
        "two write_settings in a row each reach the observer"
    );
}

#[gpui::test]
fn writing_unchanged_section_runs_no_observer(cx: &mut TestAppContext) {
    let app = TestApp::builder()
        .extension("notes", notes_extension)
        .settings(json!({ "notes": { "title": "Initial", "count": 1 } }))
        .boot(cx);

    app.write_settings(
        cx,
        json!({
            "notes": { "title": "Initial", "count": 1 },
            "other": { "flag": true }
        }),
    );

    assert!(
        cx.read(|cx| cx.global::<Probe>().observed_titles.is_empty()),
        "writing a value that does not change a section runs no observer for it"
    );
}

#[gpui::test]
fn second_boot_on_same_context_panics_before_changing_identity(cx: &mut TestAppContext) {
    let first = Application {
        product: Product {
            name: "First App",
            version: "1.0.0",
            identifier: "test.yard.first",
        },
        url_scheme: "firstapp",
        update_url: "",
        window_icon: None,
    };
    let second = Application {
        product: Product {
            name: "Second App",
            version: "2.0.0",
            identifier: "test.yard.second",
        },
        url_scheme: "secondapp",
        update_url: "",
        window_icon: None,
    };
    let expected_identity = Some(("test.yard.first".into(), "First App".into()));

    let app = TestApp::builder().application(first).boot(cx);
    assert_eq!(cx.app_identity(), expected_identity);

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _ = TestApp::builder().application(second).boot(cx);
    }))
    .expect_err("a second boot on the same TestAppContext must panic");
    let message = panic_text(&panicked);
    let lower = message.to_lowercase();
    assert!(
        message.contains("TestApp") && lower.contains("fresh") && lower.contains("context"),
        "second boot panic must name TestApp and a fresh context, got: {message:?}"
    );
    assert_eq!(
        cx.app_identity(),
        expected_identity,
        "a rejected second boot must not change cx.app_identity()"
    );

    drop(app);
    let panicked_after_drop = catch_unwind(AssertUnwindSafe(|| {
        let _ = TestApp::builder().application(second).boot(cx);
    }))
    .expect_err(
        "a second boot after dropping the first TestApp on the same context must still panic",
    );
    let after_drop_message = panic_text(&panicked_after_drop);
    let after_drop_lower = after_drop_message.to_lowercase();
    assert!(
        after_drop_message.contains("TestApp")
            && after_drop_lower.contains("fresh")
            && after_drop_lower.contains("context"),
        "second boot after drop must still name TestApp and a fresh context, got: {after_drop_message:?}"
    );
    assert_eq!(
        cx.app_identity(),
        expected_identity,
        "a rejected second boot after drop must not change cx.app_identity()"
    );
}

fn panicking_extension(_cx: &mut App, _extension: &Extension) {
    panic!("simulated extension init panic");
}

#[gpui::test]
fn second_boot_after_panicking_extension_init_panics_with_fresh_context_message(
    cx: &mut TestAppContext,
) {
    let first = Application {
        product: Product {
            name: "First App",
            version: "1.0.0",
            identifier: "test.yard.first",
        },
        url_scheme: "firstapp",
        update_url: "",
        window_icon: None,
    };
    let second = Application {
        product: Product {
            name: "Second App",
            version: "2.0.0",
            identifier: "test.yard.second",
        },
        url_scheme: "secondapp",
        update_url: "",
        window_icon: None,
    };
    let expected_identity = Some(("test.yard.first".into(), "First App".into()));

    let first_panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = TestApp::builder()
            .application(first)
            .extension("boom", panicking_extension)
            .boot(cx);
    }))
    .expect_err("panicking extension init must propagate from boot");
    assert!(
        panic_text(&first_panic).contains("simulated extension init panic"),
        "expected simulated extension init panic, got: {:?}",
        panic_text(&first_panic)
    );
    assert_eq!(
        cx.app_identity(),
        expected_identity,
        "boot sets app identity before running extension init"
    );

    let second_panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = TestApp::builder().application(second).boot(cx);
    }))
    .expect_err(
        "a second boot after a first boot stopped partway on an extension panic must panic",
    );
    let message = panic_text(&second_panic);
    let lower = message.to_lowercase();
    assert!(
        message.contains("TestApp") && lower.contains("fresh") && lower.contains("context"),
        "second boot after panicking init must name TestApp and a fresh context, got: {message:?}"
    );
    assert_eq!(
        cx.app_identity(),
        expected_identity,
        "a rejected second boot after panicking init must keep the first boot's app identity"
    );
}
