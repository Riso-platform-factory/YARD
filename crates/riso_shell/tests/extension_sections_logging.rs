use gpui::TestAppContext;
use riso_settings::Settings;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

// The target of the bridged diagnostic log records.
const PROBE: &str = "extension_sections_probe";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "riso-extension-sections-{}-{}-{nanos:x}",
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

fn settings_file(home: &TempDir, content: &str) -> PathBuf {
    let config = home.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let path = config.join("settings.json");
    fs::write(&path, content).unwrap();
    path
}

struct SpanInfo {
    label: String,
    parent: Option<u64>,
}

#[derive(Default)]
struct Captured {
    spans: BTreeMap<u64, SpanInfo>,
    entered: Vec<u64>,
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(self.0, " {}={value:?}", field.name()).unwrap();
    }
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
        let metadata = attributes.metadata();
        let mut captured = self.0.lock().unwrap();
        let id = captured.spans.len() as u64 + 1;
        let parent = captured.entered.last().copied();
        captured.spans.insert(
            id,
            SpanInfo {
                label: format!("{} {}{}", metadata.level(), metadata.name(), fields.0),
                parent,
            },
        );
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !metadata.target().starts_with("riso_settings")
            && !metadata.target().starts_with("riso_shell")
            && metadata.target() != PROBE
            || *metadata.level() == tracing::Level::TRACE
        {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let mut captured = self.0.lock().unwrap();
        // Every enclosing group, outermost first.
        let group = if captured.entered.is_empty() {
            String::from("none")
        } else {
            captured
                .entered
                .iter()
                .map(|id| captured.spans[id].label.as_str())
                .collect::<Vec<_>>()
                .join(" > ")
        };
        captured
            .lines
            .push(format!("{group}: {}{}", metadata.level(), fields.0));
    }

    fn enter(&self, id: &tracing::span::Id) {
        self.0.lock().unwrap().entered.push(id.into_u64());
    }

    fn exit(&self, id: &tracing::span::Id) {
        assert_eq!(self.0.lock().unwrap().entered.pop(), Some(id.into_u64()));
    }
}

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target().starts_with("riso_shell")
            || metadata.target().starts_with("riso_settings")
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            log::Level::Error => tracing::error!(target: PROBE, "{}", record.args()),
            log::Level::Warn => tracing::warn!(target: PROBE, "{}", record.args()),
            _ => tracing::info!(target: PROBE, "{}", record.args()),
        }
    }

    fn flush(&self) {}
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct Tour {
    greeting: String,
    size: u32,
    enabled: bool,
}

impl Default for Tour {
    fn default() -> Self {
        Self {
            greeting: "hello".into(),
            size: 14,
            enabled: true,
        }
    }
}

impl Settings for Tour {
    const KEY: &'static str = "tour";
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct Theme {
    scheme: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            scheme: "light".into(),
        }
    }
}

impl Settings for Theme {
    const KEY: &'static str = "theme";
}

#[derive(serde::Deserialize, Default)]
struct OtherTheme {}

impl Settings for OtherTheme {
    const KEY: &'static str = Theme::KEY;
}

/// Every group enclosing `line`, outermost first, joined by " > ".
fn stack_of(line: &str) -> &str {
    line.split_once(": ").map(|(stack, _)| stack).unwrap_or("")
}

/// The level and fields of `line`, without the enclosing groups.
fn event_of(line: &str) -> &str {
    line.split_once(": ")
        .map(|(_, event)| event)
        .unwrap_or(line)
}

// Alone in this binary: installs the process-wide diagnostic logger and captures every callsite.
#[gpui::test]
fn section_declarations_log_their_groups_lines_and_refusals(cx: &mut TestAppContext) {
    log::set_logger(&Diagnostics).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    let home = TempDir::new();
    let path = settings_file(
        &home,
        r#"{"tour": {"greeting": "hi", "size": 20, "enabled": false}, "theme": {"scheme": "dark"}}"#,
    );
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        cx.update(|cx| {
            riso_settings::init(cx);
            riso_settings::load(cx, path);
            // An engine registers its own section, as it always does.
            riso_settings::register::<Theme>(cx);
            // The startup wraps each extension's setup in `extension-init`.
            {
                let _init = tracing::info_span!("extension-init", extension = "tour").entered();
                let tour = riso_shell::Extension::new(cx, "tour").unwrap();
                tour.register_settings::<Tour>(cx);
                // The engine refuses the same section a second time.
                tour.register_settings::<Tour>(cx);
            }
            {
                let _init = tracing::info_span!("extension-init", extension = "guest").entered();
                let guest = riso_shell::Extension::new(cx, "guest").unwrap();
                guest.register_settings::<Tour>(cx);
            }
            {
                let _init = tracing::info_span!("extension-init", extension = "other").entered();
                let other = riso_shell::Extension::new(cx, "other").unwrap();
                other.register_settings::<OtherTheme>(cx);
            }
        });
        cx.read(|cx| {
            assert_eq!(riso_settings::get::<Tour>(cx).greeting, "hi");
            assert_eq!(riso_settings::get::<Tour>(cx).size, 20);
            assert!(!riso_settings::get::<Tour>(cx).enabled);
            assert_eq!(riso_settings::get::<Theme>(cx).scheme, "dark");
        });
    });
    let captured = trace.0.lock().unwrap();
    assert!(
        captured.entered.is_empty(),
        "spans still entered: {:?}",
        captured.entered
    );
    let labels: BTreeMap<u64, String> = captured
        .spans
        .iter()
        .map(|(id, span)| (*id, span.label.replace('"', "")))
        .collect();
    let lines: Vec<String> = captured
        .lines
        .iter()
        .map(|line| line.replace('"', ""))
        .collect();
    let init_group = |extension: &str| -> u64 {
        let label = format!("INFO extension-init extension={extension}");
        labels
            .iter()
            .find(|(_, found)| **found == label)
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("no `{label}` group in {labels:#?}"))
    };
    let tour_init = init_group("tour");
    // Only accepted declarations open a `settings-register` group: the engine's
    // own, then `tour`'s two tries inside `extension-init`. Each refusal by the
    // handle opens none.
    let registers: Vec<(Option<u64>, String)> = captured
        .spans
        .iter()
        .filter(|(_, span)| span.label.starts_with("INFO settings-register "))
        .map(|(_, span)| (span.parent, span.label.replace('"', "")))
        .collect();
    assert_eq!(
        registers,
        [
            (None, "INFO settings-register section=theme".to_string()),
            (
                Some(tour_init),
                "INFO settings-register section=tour".to_string()
            ),
            (
                Some(tour_init),
                "INFO settings-register section=tour".to_string()
            ),
        ],
        "one `settings-register` group per accepted registration in {labels:#?}"
    );
    let tour_stack = "INFO extension-init extension=tour > INFO settings-register section=tour";
    // The accepted declaration logs `registered` inside its `settings-register`
    // group; the second is refused by the engine inside a group of its own.
    let registered_at = {
        let indices: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                stack_of(line) == tour_stack
                    && event_of(line).starts_with("DEBUG")
                    && event_of(line).contains("message=registered")
            })
            .map(|(index, _)| index)
            .collect();
        let [index] = indices[..] else {
            panic!("exactly one `registered` inside `{tour_stack}` in {lines:#?}")
        };
        index
    };
    let refused_at = {
        let indices: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                stack_of(line) == tour_stack
                    && event_of(line).starts_with("ERROR")
                    && event_of(line).contains("message=section tour is already registered")
            })
            .map(|(index, _)| index)
            .collect();
        let [index] = indices[..] else {
            panic!(
                "exactly one `section tour is already registered` inside `{tour_stack}` in {lines:#?}"
            )
        };
        index
    };
    assert!(
        registered_at < refused_at,
        "the engine's refusal follows `registered` in {lines:#?}"
    );
    // A key that is not the extension's name is refused by the handle inside the
    // extension's own `extension-init`, exactly once each.
    for (extension, key) in [("guest", "tour"), ("other", "theme")] {
        let event = format!("ERROR message=extension {extension} cannot declare the section {key}");
        let found: Vec<&String> = lines
            .iter()
            .filter(|line| event_of(line) == event)
            .collect();
        assert_eq!(found.len(), 1, "exactly one `{event}` in {lines:#?}");
        assert_eq!(
            stack_of(found[0]),
            format!("INFO extension-init extension={extension}"),
            "`{event}` sits inside `extension-init` with no `settings-register` in {lines:#?}"
        );
    }
    assert_eq!(
        lines
            .iter()
            .filter(|line| event_of(line).starts_with("ERROR"))
            .count(),
        3,
        "exactly the three refusals log an `ERROR` in {lines:#?}"
    );
}
