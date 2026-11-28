//! Acceptance tests for `yard_app`'s `window_options` and `init` with a
//! non-sample `Application` (North Star).

use gpui::{App, Global, Menu, MenuItem, OwnedMenuItem, TestAppContext};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use yard_app::{Application, Init, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;
use yard_shell::{Extension, NewWindow, Quit};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-app-startup-{}-{}-{nanos:x}",
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

fn north_star(window_icon: Option<&'static [u8]>) -> Application {
    Application {
        product: Product {
            name: "North Star",
            version: "0.1.0",
            identifier: "test.yard.northstar",
        },
        url_scheme: "northstar",
        update_url: "",
        window_icon,
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn test_icon(blue: u8) -> (&'static [u8], image::RgbaImage) {
    let expected = image::RgbaImage::from_fn(4, 4, |x, y| {
        image::Rgba([(x * 60) as u8, (y * 60) as u8, blue, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    expected
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("test icon encodes as PNG");
    let png: &'static [u8] = Box::leak(bytes.into_inner().into_boxed_slice());
    (png, expected)
}

#[derive(Default)]
struct Probe {
    init_calls: usize,
    urls: Vec<String>,
}

impl Global for Probe {}

fn probe_extension(cx: &mut App, extension: &Extension) {
    cx.default_global::<Probe>().init_calls += 1;
    extension.on_open_url(cx, |url, cx| {
        cx.default_global::<Probe>().urls.push(url.to_owned());
    });
}

fn test_menus(product: &Product) -> Vec<Menu> {
    vec![
        Menu::new(product.name).items(vec![MenuItem::action(
            format!("Quit {}", product.name),
            Quit,
        )]),
        Menu::new("File").items(vec![MenuItem::action("New Window", NewWindow)]),
    ]
}

fn describe_item(item: &OwnedMenuItem) -> (&'static str, &str, &'static str) {
    match item {
        OwnedMenuItem::Separator => ("separator", "", ""),
        OwnedMenuItem::Action { name, action, .. } => ("action", name.as_str(), action.name()),
        OwnedMenuItem::Submenu(menu) => ("submenu", menu.name.as_ref(), ""),
        OwnedMenuItem::SystemMenu(menu) => ("system", menu.name.as_ref(), ""),
    }
}

#[gpui::test]
fn window_options_use_the_applications_title_app_id_and_icon(cx: &mut TestAppContext) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let (png, expected_icon) = test_icon(128);
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    let app = north_star(Some(png));
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let app = north_star(None);

    cx.update(|cx| {
        yard_ui::init(cx);
        let options = yard_app::window_options(app, cx);

        let title = options
            .titlebar
            .as_ref()
            .and_then(|bar| bar.title.as_deref());
        assert_eq!(title, Some("North Star"));
        assert_eq!(options.app_id.as_deref(), Some("test.yard.northstar"));

        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            let icon = options
                .icon
                .as_deref()
                .expect("window_options decodes the window icon on Linux and FreeBSD");
            assert_eq!(icon, &expected_icon);
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            assert!(
                options.icon.is_none(),
                "window_options leaves icon unset outside Linux and FreeBSD"
            );
        }
    });
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[gpui::test]
fn window_options_decode_each_applications_icon_independently(cx: &mut TestAppContext) {
    const INVALID_PNG: &[u8] = b"not a valid png";
    let (first_png, first_expected) = test_icon(32);
    let (second_png, second_expected) = test_icon(224);
    assert_ne!(first_expected, second_expected);

    let invalid_app = north_star(Some(INVALID_PNG));
    let first_app = north_star(Some(first_png));
    let second_app = Application {
        product: Product {
            name: "South Star",
            version: "0.2.0",
            identifier: "test.yard.southstar",
        },
        url_scheme: "southstar",
        update_url: "",
        window_icon: Some(second_png),
    };

    cx.update(|cx| {
        yard_ui::init(cx);

        let invalid = yard_app::window_options(invalid_app, cx);
        assert!(
            invalid.icon.is_none(),
            "an undecodable window icon leaves options.icon unset"
        );

        let first = yard_app::window_options(first_app, cx);
        assert_eq!(
            first.icon.as_deref(),
            Some(&first_expected),
            "a valid icon after an undecodable one still decodes"
        );

        let second = yard_app::window_options(second_app, cx);
        assert_eq!(
            second.icon.as_deref(),
            Some(&second_expected),
            "a second application gets its own decoded icon"
        );

        let first_again = yard_app::window_options(first_app, cx);
        assert_eq!(
            first_again.icon.as_deref(),
            Some(&first_expected),
            "revisiting the first application keeps its own decoded icon"
        );
    });
}

#[gpui::test]
fn init_runs_the_extension_once_sets_menus_and_uses_the_applications_url_scheme(
    cx: &mut TestAppContext,
) {
    let temp = TempDir::new();
    let paths = Paths::under(temp.path());
    let app = north_star(None);
    let extensions: &[(&'static str, Init)] = &[("probe", probe_extension)];
    let open_urls = RefCell::new(OpenUrls::default());

    cx.update(|cx| {
        yard_app::init_with_backends(
            cx,
            app,
            &paths,
            extensions,
            test_menus,
            &open_urls,
            yard_app::Backends {
                tray_backend: Some(yard_shell::TrayBackend::memory()),
                autostart_backend: Some(yard_shell::AutostartBackend::memory()),
                ..Default::default()
            },
        );

        assert_eq!(
            cx.global::<Probe>().init_calls,
            1,
            "the test extension's init runs once"
        );

        let shown = cx.get_menus().expect("the test platform keeps the menus");
        let names: Vec<&str> = shown.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["North Star", "File"]);
        let items: Vec<Vec<(&'static str, &str, &'static str)>> = shown
            .iter()
            .map(|menu| menu.items.iter().map(describe_item).collect())
            .collect();
        assert_eq!(
            items,
            vec![
                vec![("action", "Quit North Star", "yard::Quit")],
                vec![("action", "New Window", "yard::NewWindow")],
            ]
        );

        // `northstar://` reaches the extension while the sample's `yard://` scheme is refused.
        yard_shell::open_urls(
            cx,
            vec![
                "yard://probe/wrong-scheme".to_owned(),
                "northstar://probe/hello?x=1".to_owned(),
            ],
        );
        assert_eq!(
            cx.global::<Probe>().urls,
            ["northstar://probe/hello?x=1"],
            "the shell takes the application's URL scheme `northstar`"
        );
    });
}
