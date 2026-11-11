use gpui::{App, AppContext, TitlebarOptions, WindowOptions};

pub(crate) fn run(product: riso_log::Product, extensions: &[fn(&mut App)]) {
    riso_log::start(product);
    tracing::info!(product = product.name, "{} dev", product.name);
    {
        let _group = tracing::info_span!("logs-prepare").entered();
        let folder = tempfile::Builder::new()
            .prefix(&format!("{}-logs-", product.name))
            .tempdir()
            .expect("create dev logs folder")
            .keep();
        println!("Logs folder: {}", folder.display());
        std::io::Write::flush(&mut std::io::stdout()).expect("print logs folder");
        riso_log::open_files(&folder);
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
                        title: Some(format!("{} dev", product.name).into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| gpui::Empty),
            )
            .unwrap_or_else(|error| panic!("{} dev window opens: {error}", product.name));
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
