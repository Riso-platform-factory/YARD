//! `examples/north_star` is a second composition root built on `yard_app`:
//! its identity, icon, menus, and extensions belong to North Star and carry
//! nothing of the sample application.

use gpui::{Menu, MenuItem};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct TempDir(PathBuf);

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "north-star-identity-{}-{}-{nanos:x}",
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

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn application_holds_north_stars_identity_and_nothing_of_the_sample() {
    let app = north_star::application();

    assert_eq!(app.product.name, "North Star");
    assert_eq!(app.product.version, "0.1.0");
    assert_eq!(app.product.identifier, "test.yard.northstar");
    assert_eq!(app.url_scheme, "northstar");
    assert_eq!(app.update_url, "");

    assert!(
        !app.product.name.to_ascii_lowercase().contains("yard"),
        "product name contains sample name: {:?}",
        app.product.name
    );
    assert!(
        !app.product
            .identifier
            .to_ascii_lowercase()
            .contains("vn.risotech.yard"),
        "product identifier contains sample identifier: {:?}",
        app.product.identifier
    );
    assert!(
        !app.url_scheme.to_ascii_lowercase().contains("yard"),
        "url scheme contains sample name: {:?}",
        app.url_scheme
    );
    assert!(
        !app.update_url.to_ascii_lowercase().contains("yard"),
        "update url contains sample name: {:?}",
        app.update_url
    );
    assert!(
        !app.product
            .file_name()
            .to_ascii_lowercase()
            .contains("yard"),
        "derived file name contains sample name: {:?}",
        app.product.file_name()
    );
    assert_eq!(app.product.home_variable(), "NORTH_STAR_HOME");
    assert_eq!(app.product.lifecycle_variable(), "NORTH_STAR_LIFECYCLE");

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        let bytes = app
            .window_icon
            .expect("window_icon is Some on Linux and FreeBSD");
        let icon = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .expect("window_icon decodes as a PNG")
            .into_rgba8();
        assert_eq!((icon.width(), icon.height()), (256, 256));

        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let expected_source =
            repo_root.join("crates/yard_build_config/tests/configs/other/icon.png");
        let sample_source = repo_root.join("app/yard/icon.png");

        let temp = TempDir::new();
        let expected_path = temp.path().join("expected.png");
        yard_build_config::build_script::write_window_icon(&expected_source, &expected_path)
            .expect("resizing North Star's source icon succeeds");
        let expected_bytes =
            fs::read(&expected_path).expect("reading resized North Star icon succeeds");
        let expected =
            image::load_from_memory_with_format(&expected_bytes, image::ImageFormat::Png)
                .expect("resized North Star icon decodes as a PNG")
                .into_rgba8();
        assert_eq!(
            icon, expected,
            "window_icon pixels match crates/yard_build_config/tests/configs/other/icon.png resized by write_window_icon"
        );

        let sample_path = temp.path().join("sample.png");
        yard_build_config::build_script::write_window_icon(&sample_source, &sample_path)
            .expect("resizing sample icon succeeds");
        let sample_bytes = fs::read(&sample_path).expect("reading resized sample icon succeeds");
        let sample = image::load_from_memory_with_format(&sample_bytes, image::ImageFormat::Png)
            .expect("resized sample icon decodes as a PNG")
            .into_rgba8();
        assert_ne!(
            icon, sample,
            "window_icon pixels differ from app/yard/icon.png resized by write_window_icon"
        );
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        assert!(
            app.window_icon.is_none(),
            "window_icon is None outside Linux and FreeBSD"
        );
    }
}

fn assert_no_yard(menu: &Menu) {
    assert!(
        !menu.name.to_ascii_lowercase().contains("yard"),
        "menu {:?} names the sample",
        menu.name
    );
    for item in &menu.items {
        match item {
            MenuItem::Separator => {}
            MenuItem::Action { name, .. } => {
                assert!(
                    !name.to_ascii_lowercase().contains("yard"),
                    "menu item {:?} in menu {:?} names the sample",
                    name,
                    menu.name
                );
            }
            MenuItem::Submenu(submenu) => assert_no_yard(submenu),
            MenuItem::SystemMenu(system_menu) => {
                assert!(
                    !system_menu.name.to_ascii_lowercase().contains("yard"),
                    "system menu {:?} in menu {:?} names the sample",
                    system_menu.name,
                    menu.name
                );
            }
        }
    }
}

#[test]
fn menus_start_with_north_star_including_about_and_never_name_yard() {
    let product = north_star::application().product;
    let menus = north_star::menus(&product);

    let first = menus.first().expect("north_star defines at least one menu");
    assert_eq!(first.name.as_ref(), "North Star");
    let about_action = first
        .items
        .iter()
        .find_map(|item| match item {
            MenuItem::Action { name, action, .. } if name.as_ref() == "About North Star" => {
                Some(action.name())
            }
            _ => None,
        })
        .expect("the first menu includes an `About North Star` action item");
    assert_eq!(about_action, "north_star::AboutNorthStar");

    for menu in &menus {
        assert_no_yard(menu);
    }
}

#[test]
fn extensions_name_command_palette_and_north_star() {
    let names: Vec<&str> = north_star::EXTENSIONS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        names.len(),
        2,
        "expected command_palette and north_star: {names:?}"
    );
    assert!(
        names.contains(&"command_palette"),
        "EXTENSIONS does not name command_palette: {names:?}"
    );
    assert!(
        names.contains(&"north_star"),
        "EXTENSIONS does not name north_star: {names:?}"
    );
}
