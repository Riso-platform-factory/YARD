use gpui::{App, AsyncApp, WindowOptions};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
use yard_shell::yard_http::{self, HttpClient, NetworkSettings};

/// What an extension registers when the application starts.
pub type Init = fn(&mut App, &yard_shell::Extension);

#[doc(hidden)]
pub use yard_product::Product;

/// What an application gives the startup: everything it used to read at build time.
#[derive(Clone, Copy, Debug)]
pub struct Application {
    pub product: Product,
    pub url_scheme: &'static str,
    pub update_url: &'static str,
    pub window_icon: Option<&'static [u8]>,
}

/// Builds an application from the values set by the calling crate's build script.
#[macro_export]
macro_rules! application {
    () => {{
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        let window_icon = Some(&include_bytes!(concat!(env!("OUT_DIR"), "/window_icon.png"))[..]);
        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        let window_icon = None;
        $crate::Application {
            product: $crate::Product {
                name: env!("PRODUCT_NAME"),
                version: env!("PRODUCT_VERSION"),
                identifier: env!("PRODUCT_IDENTIFIER"),
            },
            url_scheme: env!("PRODUCT_URL_SCHEME"),
            update_url: env!("PRODUCT_UPDATE_URL"),
            window_icon,
        }
    }};
}

#[derive(Default, serde::Deserialize)]
#[serde(transparent)]
struct ExtensionSettings(BTreeMap<String, serde_json::Value>);

impl yard_settings::Settings for ExtensionSettings {
    const KEY: &'static str = "extensions";
}

impl ExtensionSettings {
    fn is_off(&self, name: &str) -> bool {
        self.0.get(name).and_then(serde_json::Value::as_bool) == Some(false)
    }
}

/// The URLs the system hands over, kept until the app can open them.
#[derive(Default)]
pub struct OpenUrls {
    app: Option<AsyncApp>,
    early: Vec<Vec<String>>,
}

/// Runs the application with the product's identifier in the logs.
pub fn run(
    app: Application,
    extensions: &[(&'static str, Init)],
    menus: fn(&Product) -> Vec<gpui::Menu>,
) {
    let (product, paths) = start_trace(app.product);
    let app = Application { product, ..app };
    // A second start hands its arguments over and exits before it opens the
    // log files, which belong to the running instance.
    #[cfg(not(target_os = "macos"))]
    let (instance, argument_urls) = one_instance(app, &paths);
    open_log_files(&paths);
    let extensions = extensions.to_vec();
    let platform = gpui_platform::application().with_assets(yard_ui::Assets);
    let open_urls = Rc::new(RefCell::new(OpenUrls::default()));
    // GPUI calls `on_open_urls` only on macOS, so elsewhere the URLs given
    // on the command line take the same way.
    #[cfg(not(target_os = "macos"))]
    if !argument_urls.is_empty() {
        deliver_urls(&open_urls, argument_urls);
    }
    platform.on_open_urls({
        let open_urls = open_urls.clone();
        move |urls| deliver_urls(&open_urls, urls)
    });
    platform.run(move |cx: &mut App| {
        // Windows uses the same AppUserModelID as the Start-menu shortcut
        // to group windows and the taskbar button and to post toasts under
        // the product name. Linux shows the name on notifications. macOS
        // takes the identifier and name from the bundle.
        {
            let product = app.product;
            cx.set_app_identity(product.identifier, product.name);
            tracing::debug!(
                identifier = product.identifier,
                name = product.name,
                "app identity set"
            );
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(listener) = instance {
            yard_shell::serve_instance_requests(cx, listener, app.url_scheme);
        }
        let client = init_app(cx, app, &paths, &extensions, menus, &open_urls);
        let session = yard_shell::saved_session(cx, &paths.session_file());
        yard_shell::track_session(cx, paths.session_file());
        match session {
            Some(windows) => {
                for (index, saved) in windows.iter().enumerate() {
                    let window_file = (index == 0).then(|| paths.window_file());
                    open_session_window(cx, app, saved, window_file);
                }
            }
            None => {
                open_window(cx, app, Some(paths.window_file()), None);
            }
        }
        start_update_check(cx, app, client);
        yard_log::startup_finished();
    });
}

/// Starts both logs and opens their files in the product's logs directory,
/// and returns the checked product with its directories.
pub fn start_logs(product: Product) -> (Product, yard_paths::Paths) {
    let (product, paths) = start_trace(product);
    open_log_files(&paths);
    (product, paths)
}

/// Starts both logs, and returns the checked product with its directories.
fn start_trace(product: Product) -> (Product, yard_paths::Paths) {
    // The logs start from the build values as they are, since `start` needs
    // the product; the product is then checked once the trace listens, so
    // its `product` group shows in `app-startup`.
    yard_log::start(product);
    let product = Product::new(product.name, product.version, product.identifier);
    let paths = yard_paths::Paths::from_env(&product);
    (product, paths)
}

/// Opens the files of both logs in the product's logs directory.
fn open_log_files(paths: &yard_paths::Paths) {
    let _group = tracing::info_span!("logs-prepare").entered();
    yard_log::open_files(paths.logs_dir());
}

/// Hands the command line's URLs to the instance already running and exits,
/// or else returns this start's listener, if it is the one instance, with
/// the URLs. macOS keeps one instance and hands URLs over itself.
#[cfg(not(target_os = "macos"))]
fn one_instance(
    app: Application,
    paths: &yard_paths::Paths,
) -> (Option<yard_shell::InstanceListener>, Vec<String>) {
    let product = app.product;
    let (instance, urls) = yard_shell::start_instance(
        paths,
        product.identifier,
        std::env::args_os().skip(1),
        app.url_scheme,
    );
    let listener = match instance {
        yard_shell::Instance::Forwarded => yard_log::exit(0),
        yard_shell::Instance::Refused(reason) => {
            eprintln!("{} is already running: {reason}", product.name);
            yard_log::exit(1)
        }
        yard_shell::Instance::Listening(listener) => Some(listener),
        yard_shell::Instance::Alone(_) => None,
    };
    (listener, urls)
}

/// Opens URLs once the extensions are loaded: the system's on macOS, and
/// elsewhere those of the command line.
fn deliver_urls(open_urls: &RefCell<OpenUrls>, urls: Vec<String>) {
    let mut listener = open_urls.borrow_mut();
    if let Some(app) = &listener.app {
        app.spawn(async move |cx| {
            cx.update(|cx| yard_shell::open_urls(cx, urls));
        })
        .detach();
    } else {
        listener.early.push(urls);
    }
}

/// Sets up the app as the application does before it opens a window: the
/// settings, the theme, the shell, the extensions, the keymap, and the menus.
pub fn init(
    cx: &mut App,
    app: Application,
    paths: &yard_paths::Paths,
    extensions: &[(&'static str, Init)],
    menus: fn(&Product) -> Vec<gpui::Menu>,
    open_urls: &RefCell<OpenUrls>,
) {
    init_app(cx, app, paths, extensions, menus, open_urls);
}

fn init_app(
    cx: &mut App,
    app: Application,
    paths: &yard_paths::Paths,
    extensions: &[(&'static str, Init)],
    menus: fn(&Product) -> Vec<gpui::Menu>,
    open_urls: &RefCell<OpenUrls>,
) -> Option<HttpClient> {
    let client = {
        let _group = tracing::info_span!("app-init").entered();
        yard_log::init_app(cx);
        yard_settings::init(cx);
        yard_settings::load(cx, paths.settings_file());
        yard_settings::watch(cx);
        yard_ui::init(cx);
        yard_ui::load_user_themes(cx, &paths.themes_dir());
        yard_ui::apply_theme_setting(cx);
        yard_shell::set_window_title(cx, app.product.name);
        yard_shell::set_url_scheme(cx, app.url_scheme);
        yard_shell::set_layout_file(cx, paths.layout_file());
        yard_shell::set_state_file(cx, paths.state_file());
        yard_shell::set_store_dir(cx, paths.store_dir());
        yard_shell::set_data_file(cx, paths.data_file());
        yard_settings::register::<NetworkSettings>(cx);
        let product = app.product;
        let agent = yard_http::user_agent(product.name, product.version);
        let client = HttpClient::new(&agent, yard_settings::get::<NetworkSettings>(cx))
            .inspect_err(|error| log::error!("HTTP client failed to start: {error}"))
            .ok();
        let shell_client = client.clone().unwrap_or_else(HttpClient::unavailable);
        yard_shell::set_http_client(cx, shell_client.clone());
        cx.set_http_client(Arc::new(shell_client.for_caller("gpui")));
        yard_settings::observe::<NetworkSettings>(cx, move |cx| {
            shell_client.reconfigure(yard_settings::get::<NetworkSettings>(cx));
        })
        .detach();
        let named_paths = paths.clone();
        yard_shell::set_named_layout_files(cx, move |name| named_paths.named_layout_file(name));
        yard_shell::bind_default_keys(cx);
        yard_ui::bind_text_input_keys(cx);
        yard_shell::handle_quit(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        client
    };
    let early_urls = std::mem::take(&mut open_urls.borrow_mut().early);
    for urls in early_urls {
        yard_shell::open_urls(cx, urls);
    }
    {
        yard_settings::register::<ExtensionSettings>(cx);
        let settings = yard_settings::get::<ExtensionSettings>(cx);
        let (off, on): (Vec<_>, Vec<_>) = extensions
            .iter()
            .map(|&(name, _)| name)
            .partition(|name| settings.is_off(name));
        let _group =
            tracing::info_span!("extensions-load", count = extensions.len(), off = off.len())
                .entered();
        for (name, value) in &settings.0 {
            if !value.is_boolean() {
                log::warn!("extension {name:?} in the extensions setting is not true or false");
            } else if !extensions.iter().any(|(known, _)| known == name) {
                log::warn!("unknown extension {name:?} in the extensions setting");
            }
        }
        yard_shell::set_extensions_off(cx, !off.is_empty());
        for &(name, init) in extensions {
            let _group = tracing::info_span!("extension-init", extension = name).entered();
            if off.contains(&name) {
                tracing::info!("turned off");
                continue;
            }
            if let Some(extension) = yard_shell::Extension::new(cx, name) {
                init(cx, &extension);
            }
        }
        yard_settings::observe::<ExtensionSettings>(cx, move |cx| {
            let settings = yard_settings::get::<ExtensionSettings>(cx);
            let names = on
                .iter()
                .copied()
                .filter(|name| settings.is_off(name))
                .chain(off.iter().copied().filter(|name| !settings.is_off(name)))
                .collect::<Vec<_>>();
            if !names.is_empty() {
                let _group = tracing::info_span!("extensions-setting-changed", ?names).entered();
                tracing::info!("applies at the next start");
            }
        })
        .detach();
    }
    // Hand over any URLs that arrived during extension initialization.
    // GPUI queues subsequent deliveries on the main thread in callback order.
    let early_urls = {
        let mut listener = open_urls.borrow_mut();
        listener.app = Some(cx.to_async());
        std::mem::take(&mut listener.early)
    };
    for urls in early_urls {
        yard_shell::open_urls(cx, urls);
    }
    yard_shell::extensions_loaded(cx);
    yard_keymap::load_user_keymap(cx, &paths.keymap_file());
    yard_keymap::watch_user_keymap(cx, paths.keymap_file());
    yard_shell::set_menus(cx, menus(&app.product));
    yard_shell::set_window_opener(cx, move |cx| {
        open_window(cx, app, None, None);
    });
    client
}

/// Starts checking the product's update address, when it names one, and
/// installing what it finds, through an extension handle named `update` for
/// the toasts.
fn start_update_check(cx: &mut App, app: Application, client: Option<HttpClient>) {
    let product = app.product;
    let url = app.update_url;
    if url.is_empty() {
        tracing::debug!("no update address");
        return;
    }
    let Some(client) = client else {
        return;
    };
    let Some(extension) = yard_shell::Extension::new(cx, "update") else {
        return;
    };
    yard_update::start(
        cx,
        &extension,
        product,
        url,
        Arc::new(client.for_caller("update")),
    );
}

/// The options every window of the product starts from: its title bar, its
/// `app_id`, which Wayland and X11 match with the product's `.desktop` file,
/// and on Linux and FreeBSD its X11 icon.
pub fn window_options(app: Application, cx: &App) -> WindowOptions {
    let product = app.product;
    let window_background = yard_ui::window_background(cx);
    tracing::debug!(app_id = product.identifier, background = ?window_background, "window options");
    WindowOptions {
        titlebar: Some(yard_ui::titlebar_options(product.name)),
        window_background,
        app_id: Some(product.identifier.to_owned()),
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        icon: app.window_icon.and_then(app_icon),
        // YARD draws its own frame and controls on Linux, as Zed does.
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        window_decorations: Some(gpui::WindowDecorations::Client),
        ..Default::default()
    }
}

/// Decodes the application's PNG for this window.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn app_icon(png: &[u8]) -> Option<Arc<image::RgbaImage>> {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map(|icon| Arc::new(icon.into_rgba8()))
        // `build.rs` wrote this PNG with the same crate, so a failure is a bug.
        .inspect_err(|error| tracing::error!("the window icon does not decode: {error}"))
        .ok()
}

/// Opens a window of the product at `bounds`, or else at the bounds saved in
/// `window_file`, which it then keeps up to date.
pub fn open_window(
    cx: &mut App,
    app: Application,
    window_file: Option<std::path::PathBuf>,
    bounds: Option<gpui::WindowBounds>,
) -> gpui::AnyWindowHandle {
    let _group = tracing::info_span!("window-open").entered();
    let saved = window_file
        .as_ref()
        .and_then(|file| yard_shell::saved_window_bounds(cx, file));
    let options = window_options(app, cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: bounds.or(saved.map(|(bounds, _)| bounds)),
                display_id: saved.map(|(_, display)| display),
                ..options
            },
            |window, cx| {
                yard_ui::follow_appearance(window, cx);
                yard_shell::new_shell(window, cx)
            },
        )
        .unwrap_or_else(|error| panic!("{} window opens: {error}", app.product.name));
    window
        .update(cx, |_, window, cx| {
            if let Some(file) = window_file {
                yard_shell::track_window_bounds(window, cx, file);
            }
            window.activate_window();
        })
        .unwrap_or_else(|error| panic!("{} window is open: {error}", app.product.name));
    cx.activate(true);
    window.into()
}

/// Opens a window of the saved session with its bounds and layers; the
/// main window, the first, also keeps `window.json` up to date.
fn open_session_window(
    cx: &mut App,
    app: Application,
    saved: &yard_shell::SessionWindow,
    window_file: Option<std::path::PathBuf>,
) {
    let _group = tracing::info_span!("window-open").entered();
    let options = window_options(app, cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: saved.bounds,
                display_id: saved.display,
                ..options
            },
            |window, cx| {
                yard_ui::follow_appearance(window, cx);
                yard_shell::new_session_shell(window, cx, saved)
            },
        )
        .unwrap_or_else(|error| panic!("{} window opens: {error}", app.product.name));
    window
        .update(cx, |_, window, cx| {
            if let Some(file) = window_file {
                yard_shell::track_window_bounds(window, cx, file);
            }
            window.activate_window();
        })
        .unwrap_or_else(|error| panic!("{} window is open: {error}", app.product.name));
    cx.activate(true);
}
