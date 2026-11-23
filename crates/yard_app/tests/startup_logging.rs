//! `yard_app::init` opens `app-init`, `extensions-load{count=1}`, and
//! `extension-init{extension=…}` at `INFO`, and logs no `ERROR`.
//!
//! This test captures `tracing` output and installs a process-wide diagnostic
//! logger, so it is alone in its file.

use gpui::{App, Menu, MenuItem, TestAppContext};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use yard_app::{Application, Init, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;
use yard_shell::{Extension, Quit};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "yard-app-startup-logging-{}-{}-{nanos:x}",
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

static DIAGNOSTIC_INIT: Once = Once::new();
static DIAGNOSTIC_ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("yard")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) && record.level() == log::Level::Error {
            DIAGNOSTIC_ERRORS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(format!("[{}] {}", record.target(), record.args()));
        }
    }

    fn flush(&self) {}
}

#[derive(Debug)]
enum Record {
    Group {
        id: tracing::span::Id,
        parent: Option<tracing::span::Id>,
        target: &'static str,
        level: tracing::Level,
        name: &'static str,
        fields: Vec<(String, String)>,
    },
    Line {
        group: Option<tracing::span::Id>,
        target: &'static str,
        level: tracing::Level,
        fields: Vec<(String, String)>,
    },
}

#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().into(), value.into()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let debug = format!("{value:?}");
        let value = debug
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .unwrap_or(debug.as_str());
        self.0.push((field.name().into(), value.into()));
    }
}

#[derive(Clone, Default)]
struct Trace {
    records: Arc<Mutex<Vec<Record>>>,
    open: Arc<Mutex<Vec<tracing::span::Id>>>,
    entered: Arc<Mutex<Vec<tracing::span::Id>>>,
}

impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let parent = self.open.lock().unwrap().last().cloned();
        let mut records = self.records.lock().unwrap();
        let id = tracing::span::Id::from_u64(records.len() as u64 + 1);
        records.push(Record::Group {
            id: id.clone(),
            parent,
            target: attributes.metadata().target(),
            level: *attributes.metadata().level(),
            name: attributes.metadata().name(),
            fields: fields.0,
        });
        id
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let group = self.open.lock().unwrap().last().cloned();
        self.records.lock().unwrap().push(Record::Line {
            group,
            target: event.metadata().target(),
            level: *event.metadata().level(),
            fields: fields.0,
        });
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.open.lock().unwrap().push(id.clone());
        self.entered.lock().unwrap().push(id.clone());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.open.lock().unwrap().pop().as_ref(), Some(id));
    }
}

fn probe_extension(_: &mut App, _: &Extension) {
    tracing::debug!(target: "startup_logging_probe", "probe extension initialized");
}

fn test_menus(product: &Product) -> Vec<Menu> {
    vec![Menu::new(product.name).items(vec![MenuItem::action(
        format!("Quit {}", product.name),
        Quit,
    )])]
}

#[gpui::test]
fn init_opens_startup_groups_at_info_and_logs_no_error(cx: &mut TestAppContext) {
    DIAGNOSTIC_INIT.call_once(|| {
        log::set_logger(&Diagnostics).unwrap();
        log::set_max_level(log::LevelFilter::Trace);
    });
    DIAGNOSTIC_ERRORS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clear();

    let temp = TempDir::new();
    let paths = Paths::under(temp.path());
    let app = Application {
        product: Product {
            name: "North Star",
            version: "0.1.0",
            identifier: "test.yard.northstar",
        },
        url_scheme: "northstar",
        update_url: "",
        window_icon: None,
    };
    let extensions: &[(&'static str, Init)] = &[("probe", probe_extension)];
    let open_urls = RefCell::new(OpenUrls::default());

    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            yard_app::init(cx, app, &paths, extensions, test_menus, &open_urls);
        });
    });

    assert!(
        trace.open.lock().unwrap().is_empty(),
        "every opened group was exited"
    );

    let records = trace.records.lock().unwrap();
    let entered = trace.entered.lock().unwrap();

    let trace_errors: Vec<&Record> = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                Record::Line {
                    level: tracing::Level::ERROR,
                    ..
                }
            )
        })
        .collect();
    assert!(
        trace_errors.is_empty(),
        "init logged tracing ERRORs: {trace_errors:#?}"
    );

    let diagnostic_errors = DIAGNOSTIC_ERRORS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    assert!(
        diagnostic_errors.is_empty(),
        "init logged diagnostic ERRORs: {diagnostic_errors:#?}"
    );

    let find_startup_group = |expected_name: &str| {
        let mut matches = records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| match record {
                Record::Group {
                    id,
                    parent,
                    target,
                    level,
                    name,
                    fields,
                } if *name == expected_name && target.starts_with("yard_app") => {
                    Some((index, id.clone(), parent.clone(), *level, fields.as_slice()))
                }
                _ => None,
            });
        let found = matches
            .next()
            .unwrap_or_else(|| panic!("missing `{expected_name}` group in trace: {records:#?}"));
        assert!(
            matches.next().is_none(),
            "expected exactly one `{expected_name}` group in trace: {records:#?}"
        );
        found
    };

    let (app_init_index, app_init_id, app_init_parent, app_init_level, app_init_fields) =
        find_startup_group("app-init");
    assert_eq!(app_init_level, tracing::Level::INFO);
    assert!(app_init_parent.is_none(), "`app-init` is a top-level group");
    assert!(
        app_init_fields.is_empty(),
        "`app-init` has no fields: {app_init_fields:?}"
    );
    assert!(
        entered.contains(&app_init_id),
        "`app-init` was entered for the work"
    );
    assert!(
        records.iter().any(|record| matches!(
            record,
            Record::Group { parent: Some(parent), .. } if *parent == app_init_id
        )),
        "`app-init` surrounded initialization work: {records:#?}"
    );

    fn fields_as_pairs(fields: &[(String, String)]) -> Vec<(&str, &str)> {
        fields
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }

    let (
        extensions_load_index,
        extensions_load_id,
        extensions_load_parent,
        extensions_load_level,
        extensions_load_fields,
    ) = find_startup_group("extensions-load");
    assert_eq!(extensions_load_level, tracing::Level::INFO);
    assert!(
        extensions_load_parent.is_none(),
        "`extensions-load` is a top-level group"
    );
    assert_eq!(
        fields_as_pairs(extensions_load_fields),
        [("count", "1")],
        "`extensions-load` records count=1"
    );
    assert!(
        extensions_load_index > app_init_index,
        "`extensions-load` opens after `app-init`"
    );
    assert!(
        entered.contains(&extensions_load_id),
        "`extensions-load` was entered for the work"
    );

    let (
        extension_init_index,
        extension_init_id,
        extension_init_parent,
        extension_init_level,
        extension_init_fields,
    ) = find_startup_group("extension-init");
    assert_eq!(extension_init_level, tracing::Level::INFO);
    assert_eq!(
        extension_init_parent.as_ref(),
        Some(&extensions_load_id),
        "`extension-init` is nested inside `extensions-load`"
    );
    assert_eq!(
        fields_as_pairs(extension_init_fields),
        [("extension", "probe")],
        "`extension-init` records extension=probe"
    );
    assert!(
        extension_init_index > extensions_load_index,
        "`extension-init` opens inside `extensions-load`"
    );
    assert!(
        entered.contains(&extension_init_id),
        "`extension-init` was entered for the work"
    );
    assert!(
        records.iter().any(|record| matches!(
            record,
            Record::Line {
                group: Some(group),
                target: "startup_logging_probe",
                fields,
                ..
            } if *group == extension_init_id
                && fields
                    .iter()
                    .any(|(key, value)| key == "message" && value == "probe extension initialized")
        )),
        "`extension-init` surrounded the probe extension's init: {records:#?}"
    );
}
