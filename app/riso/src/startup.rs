use gpui::{App, AsyncApp, WindowOptions};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Init = fn(&mut App, &riso_shell::Extension);

use riso_product::Product;

#[derive(Default)]
struct OpenUrls {
    app: Option<AsyncApp>,
    early: Vec<Vec<String>>,
}

/// Runs the application with the product's identifier in the logs.
pub(crate) fn run(
    product: Product,
    extensions: &[(&'static str, Init)],
    menus: fn(&Product) -> Vec<gpui::Menu>,
) {
    // The logs start from the build values as they are, since `start` needs
    // the product; the product is then checked once the trace listens, so
    // its `product` group shows in `app-startup`.
    riso_log::start(product);
    let product = Product::new(product.name, product.version, product.identifier);
    let paths = riso_paths::Paths::from_env(&product);
    {
        let _group = tracing::info_span!("logs-prepare").entered();
        riso_log::open_files(paths.logs_dir());
    }
    let extensions = extensions.to_vec();
    let app = gpui_platform::application().with_assets(riso_ui::Assets);
    let open_urls = Rc::new(RefCell::new(OpenUrls::default()));
    app.on_open_urls({
        let open_urls = open_urls.clone();
        move |urls| {
            let mut listener = open_urls.borrow_mut();
            if let Some(app) = &listener.app {
                app.spawn(async move |cx| {
                    cx.update(|cx| riso_shell::open_urls(cx, urls));
                })
                .detach();
            } else {
                listener.early.push(urls);
            }
        }
    });
    app.run(move |cx: &mut App| {
        {
            let _group = tracing::info_span!("app-init").entered();
            riso_log::init_app(cx);
            riso_settings::init(cx);
            riso_settings::load(cx, paths.settings_file());
            riso_settings::watch(cx);
            riso_ui::init(cx);
            riso_ui::load_user_themes(cx, &paths.themes_dir());
            riso_ui::apply_theme_setting(cx);
            riso_shell::set_window_title(cx, product.name);
            riso_shell::set_url_scheme(cx, env!("PRODUCT_URL_SCHEME"));
            riso_shell::set_layout_file(cx, paths.layout_file());
            riso_shell::set_state_file(cx, paths.state_file());
            riso_shell::set_store_dir(cx, paths.store_dir());
            let named_paths = paths.clone();
            riso_shell::set_named_layout_files(cx, move |name| named_paths.named_layout_file(name));
            riso_shell::bind_default_keys(cx);
            riso_ui::bind_text_input_keys(cx);
            riso_shell::handle_quit(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        }
        let early_urls = std::mem::take(&mut open_urls.borrow_mut().early);
        for urls in early_urls {
            riso_shell::open_urls(cx, urls);
        }
        {
            let _group = tracing::info_span!("extensions-load", count = extensions.len()).entered();
            for (name, init) in extensions {
                let _group = tracing::info_span!("extension-init", extension = name).entered();
                if let Some(extension) = riso_shell::Extension::new(cx, name) {
                    init(cx, &extension);
                }
            }
        }
        // Hand over any URLs that arrived during extension initialization.
        // GPUI queues subsequent deliveries on the main thread in callback order.
        let early_urls = {
            let mut listener = open_urls.borrow_mut();
            listener.app = Some(cx.to_async());
            std::mem::take(&mut listener.early)
        };
        for urls in early_urls {
            riso_shell::open_urls(cx, urls);
        }
        riso_shell::extensions_loaded(cx);
        riso_keymap::load_user_keymap(cx, &paths.keymap_file());
        riso_keymap::watch_user_keymap(cx, paths.keymap_file());
        riso_shell::set_menus(cx, menus(&product));
        riso_shell::set_window_opener(cx, move |cx| {
            open_window(cx, product, None);
        });
        let session = riso_shell::saved_session(cx, &paths.session_file());
        riso_shell::track_session(cx, paths.session_file());
        match session {
            Some(windows) => {
                for (index, saved) in windows.iter().enumerate() {
                    let window_file = (index == 0).then(|| paths.window_file());
                    open_session_window(cx, product, saved, window_file);
                }
            }
            None => open_window(cx, product, Some(paths.window_file())),
        }
        start_update_check(cx, product);
        riso_log::startup_finished();
    });
}

/// Starts checking the product's update address, when it names one, through
/// an extension handle named `update` for the toast.
fn start_update_check(cx: &mut App, product: Product) {
    let url = env!("PRODUCT_UPDATE_URL");
    if url.is_empty() {
        tracing::debug!("no update address");
        return;
    }
    let Some(extension) = riso_shell::Extension::new(cx, "update") else {
        return;
    };
    let user_agent = format!("{}/{}", product.name, product.version);
    match reqwest_client::ReqwestClient::user_agent(&user_agent) {
        Ok(client) => riso_update::start(cx, &extension, product, url, Arc::new(client)),
        Err(error) => tracing::warn!("no update check: the HTTP client failed to start: {error:#}"),
    }
}

fn open_window(cx: &mut App, product: Product, window_file: Option<std::path::PathBuf>) {
    let _group = tracing::info_span!("window-open").entered();
    let saved = window_file
        .as_ref()
        .and_then(|file| riso_shell::saved_window_bounds(cx, file));
    let window = cx
        .open_window(
            WindowOptions {
                titlebar: Some(riso_ui::titlebar_options(product.name)),
                window_bounds: saved.map(|(bounds, _)| bounds),
                display_id: saved.map(|(_, display)| display),
                ..Default::default()
            },
            |window, cx| {
                riso_ui::follow_appearance(window, cx);
                riso_shell::new_shell(window, cx)
            },
        )
        .unwrap_or_else(|error| panic!("{} window opens: {error}", product.name));
    window
        .update(cx, |_, window, cx| {
            if let Some(file) = window_file {
                riso_shell::track_window_bounds(window, cx, file);
            }
            window.activate_window();
        })
        .unwrap_or_else(|error| panic!("{} window is open: {error}", product.name));
    cx.activate(true);
}

/// Opens a window of the saved session with its bounds and layers; the
/// main window, the first, also keeps `window.json` up to date.
fn open_session_window(
    cx: &mut App,
    product: Product,
    saved: &riso_shell::SessionWindow,
    window_file: Option<std::path::PathBuf>,
) {
    let _group = tracing::info_span!("window-open").entered();
    let window = cx
        .open_window(
            WindowOptions {
                titlebar: Some(riso_ui::titlebar_options(product.name)),
                window_bounds: saved.bounds,
                display_id: saved.display,
                ..Default::default()
            },
            |window, cx| {
                riso_ui::follow_appearance(window, cx);
                riso_shell::new_session_shell(window, cx, saved)
            },
        )
        .unwrap_or_else(|error| panic!("{} window opens: {error}", product.name));
    window
        .update(cx, |_, window, cx| {
            if let Some(file) = window_file {
                riso_shell::track_window_bounds(window, cx, file);
            }
            window.activate_window();
        })
        .unwrap_or_else(|error| panic!("{} window is open: {error}", product.name));
    cx.activate(true);
}
