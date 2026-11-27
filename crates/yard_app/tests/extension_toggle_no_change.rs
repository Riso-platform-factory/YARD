use gpui::TestAppContext;
use std::{
    cell::RefCell,
    fs,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::{Metadata, Subscriber, span};
use yard_app::{Application, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<&'static str>>>);

impl Subscriber for Trace {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        matches!(
            metadata.name(),
            "settings-reload" | "extensions-setting-changed"
        )
    }

    fn new_span(&self, attrs: &span::Attributes<'_>) -> span::Id {
        let mut names = self.0.lock().unwrap();
        names.push(attrs.metadata().name());
        span::Id::from_u64(names.len() as u64)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

#[gpui::test]
fn reload_without_an_effective_extension_change_has_no_next_start_group(cx: &mut TestAppContext) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("yard-toggle-{}-{unique}", std::process::id()));
    let paths = Paths::under(&home);
    let file = paths.settings_file();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let trace = Trace::default();

    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
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
                &[("known", |_, _| {})],
                |_| Vec::new(),
                &RefCell::new(OpenUrls::default()),
            );
        });
        cx.run_until_parked();
        fs::write(&file, r#"{"extensions":{"known":true,"misspelled":false}}"#).unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.quit();
    });

    fs::remove_dir_all(home).unwrap();
    assert_eq!(*trace.0.lock().unwrap(), ["settings-reload"]);
}
