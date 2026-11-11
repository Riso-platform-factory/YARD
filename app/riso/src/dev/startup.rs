use gpui::{App, AppContext, TitlebarOptions, WindowOptions};

pub(crate) fn run(product: riso_log::Product, extensions: &[fn(&mut App)]) {
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
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
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
                |_, cx| cx.new(|_| gpui::Empty),
            )
            .unwrap_or_else(|error| panic!("{} main window opens: {error}", product.name));
            cx.activate(true);
        }
        {
            let _group = tracing::info_span!("extensions-load", count = extensions.len()).entered();
            for init in extensions {
                init(cx);
            }
        }
        riso_log::startup_finished();
    });
}
