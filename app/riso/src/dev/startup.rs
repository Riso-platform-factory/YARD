use gpui::{App, TitlebarOptions, WindowOptions};

type Init = fn(&mut App, &riso_shell::dev::Extension);

pub(crate) fn run(product: riso_log::Product, extensions: &[(&'static str, Init)]) {
    riso_log::start(product);
    let paths = riso_paths::Paths::from_env(product.name);
    {
        let _group = tracing::info_span!("logs-prepare").entered();
        riso_log::open_files(paths.logs_dir());
    }
    let extensions = extensions.to_vec();
    gpui_platform::application()
        .with_assets(riso_ui::Assets)
        .run(move |cx: &mut App| {
            {
                let _group = tracing::info_span!("app-init").entered();
                riso_log::init_app(cx);
                riso_settings::init(cx);
                riso_settings::load(cx, paths.settings_file());
                riso_settings::watch(cx);
                riso_ui::init(cx);
                riso_ui::apply_theme_setting(cx);
                riso_shell::dev::set_layout_file(cx, paths.layout_file());
                cx.on_window_closed(|cx, _| {
                    if cx.windows().is_empty() {
                        cx.quit();
                    }
                })
                .detach();
            }
            {
                let _group =
                    tracing::info_span!("extensions-load", count = extensions.len()).entered();
                for (name, init) in extensions {
                    let _group = tracing::info_span!("extension-init", extension = name).entered();
                    if let Some(extension) = riso_shell::dev::Extension::new(cx, name) {
                        init(cx, &extension);
                    }
                }
            }
            {
                let _group = tracing::info_span!("window-open").entered();
                cx.open_window(
                    WindowOptions {
                        titlebar: Some(TitlebarOptions {
                            title: Some(product.name.into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        riso_ui::follow_appearance(window, cx);
                        riso_shell::dev::new_shell(window, cx)
                    },
                )
                .unwrap_or_else(|error| panic!("{} main window opens: {error}", product.name));
                cx.activate(true);
            }
            riso_log::startup_finished();
        });
}
