use gpui::{App, TitlebarOptions, WindowOptions};

type Init = fn(&mut App, &riso_shell::Extension);

pub(crate) fn run(product: riso_log::Product, extensions: &[(&'static str, Init)]) {
    riso_log::start(product);
    let paths = riso_paths::Paths::from_env(product.name);
    {
        let _group = tracing::info_span!("logs-prepare").entered();
        riso_log::open_files(paths.logs_dir());
    }
    let extensions = extensions.to_vec();
    gpui_platform::application().run(move |cx: &mut App| {
        {
            let _group = tracing::info_span!("app-init").entered();
            riso_log::init_app(cx);
            riso_ui::init(cx);
            riso_settings::dev::init(cx);
            riso_settings::dev::load(cx, paths.settings_file());
            riso_settings::dev::watch(cx);
            riso_shell::set_layout_file(cx, paths.layout_file());
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
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
                riso_shell::new_shell,
            )
            .unwrap_or_else(|error| panic!("{} main window opens: {error}", product.name));
            cx.activate(true);
        }
        riso_log::startup_finished();
    });
}
