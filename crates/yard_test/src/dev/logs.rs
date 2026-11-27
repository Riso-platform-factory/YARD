use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, ThreadId},
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::Interest,
};
use tracing_core::{span::Current, subscriber::NoSubscriber};

static CAPTURE_LOCK: Mutex<()> = Mutex::new(());
static INSTALL_LOCK: Mutex<()> = Mutex::new(());
static INSTALL_ROUTER: OnceLock<Result<(), tracing::dispatcher::SetGlobalDefaultError>> =
    OnceLock::new();
static NEXT_GROUP_ID: AtomicU64 = AtomicU64::new(1);
static INSTALL_LOGGER: OnceLock<Result<(), log::SetLoggerError>> = OnceLock::new();
static LOGGER: DiagnosticLogger = DiagnosticLogger(Mutex::new(None));

thread_local! {
    static RECORDING: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

/// Captures tracing on the calling thread. Only one capture may run on a thread at a time.
/// Boot a [`TestApp`](crate::TestApp) before capturing a single operation.
///
/// Drop span guards on the thread that entered them. Dropping a guard elsewhere leaves its
/// span current on the entering thread for the rest of the capture. Use spans from the
/// capture's router. Spans from other subscribers are unsupported.
///
/// Captures panic inside another subscriber's `with_default`. A binary that uses captures
/// must also avoid using `tracing::Dispatch::none()` as a scoped default on any thread,
/// since callsites first reached there can be disabled in later captures.
pub fn capture_trace<R>(f: impl FnOnce(&Trace) -> R) -> R {
    capture(false, f)
}

/// Captures tracing and diagnostic logs on the calling thread. Only one capture may run
/// on a thread at a time. Boot a [`TestApp`](crate::TestApp) before capturing a single operation.
/// The dispatcher and span guard restrictions of [`capture_trace`] also apply here.
pub fn capture_logs<R>(f: impl FnOnce(&Trace) -> R) -> R {
    capture(true, f)
}

fn capture<R>(diagnostics: bool, f: impl FnOnce(&Trace) -> R) -> R {
    assert!(
        RECORDING.with(|recording| recording.borrow().is_none()),
        "yard_test captures cannot be nested on the same thread"
    );
    install(diagnostics);
    assert!(
        tracing::dispatcher::get_default(|dispatch| dispatch.is::<Router>()),
        "yard_test cannot capture inside another subscriber's with_default"
    );
    let _lock = lock(&CAPTURE_LOCK);
    let trace = Trace::default();
    let _diagnostics = diagnostics.then(|| DiagnosticScope::new(&trace));
    RECORDING.with(|recording| *recording.borrow_mut() = Some(Recorder(trace.spans.clone())));
    let _scope = TraceScope;
    f(&trace)
}

fn install(diagnostics: bool) {
    // Serialize the conflict check with publication of the kit's own router.
    let _lock = lock(&INSTALL_LOCK);
    if diagnostics {
        if tracing::dispatcher::has_been_set() && !matches!(INSTALL_ROUTER.get(), Some(Ok(()))) {
            // has_been_set also counts scoped defaults. A fresh thread sees only the global one.
            let has_global_subscriber =
                thread::spawn(|| tracing::dispatcher::get_default(|d| !d.is::<NoSubscriber>()))
                    .join()
                    .expect("check the global tracing subscriber");
            assert!(
                !has_global_subscriber,
                "the binary already has a global tracing subscriber"
            );
        }
        INSTALL_LOGGER
            .get_or_init(|| log::set_logger(&LOGGER))
            .as_ref()
            .expect("the binary already has a logger");
    }
    INSTALL_ROUTER
        .get_or_init(|| {
            tracing::dispatcher::set_global_default(tracing::Dispatch::new(Router))?;
            tracing::callsite::rebuild_interest_cache();
            Ok(())
        })
        .as_ref()
        .expect("the binary already has a global tracing subscriber");
}

struct TraceScope;

impl Drop for TraceScope {
    fn drop(&mut self) {
        RECORDING.with(|recording| *recording.borrow_mut() = None);
    }
}

fn with_recording<R>(f: impl FnOnce(Recorder) -> R) -> Option<R> {
    RECORDING
        .try_with(|recording| recording.borrow().clone())
        .ok()
        .flatten()
        .map(f)
}

struct Router;

impl Subscriber for Router {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _: &Metadata<'_>) -> bool {
        RECORDING
            .try_with(|recording| recording.borrow().is_some())
            .unwrap_or(false)
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        with_recording(|recording| recording.new_span(attributes)).unwrap_or_else(next_group_id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        with_recording(|recording| recording.record(span, values));
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        with_recording(|recording| recording.event(event));
    }

    fn enter(&self, span: &Id) {
        with_recording(|recording| recording.enter(span));
    }

    fn exit(&self, span: &Id) {
        with_recording(|recording| recording.exit(span));
    }

    fn current_span(&self) -> Current {
        with_recording(|recording| recording.current_span()).unwrap_or_else(Current::none)
    }
}

fn next_group_id() -> Id {
    Id::from_u64(NEXT_GROUP_ID.fetch_add(1, Ordering::Relaxed))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
pub struct Trace {
    spans: Arc<Mutex<RecordedTrace>>,
    diagnostics: Arc<Mutex<Vec<DiagnosticRecord>>>,
}

impl Trace {
    pub fn groups(&self) -> Vec<TraceGroup> {
        lock(&self.spans)
            .groups
            .iter()
            .map(|(group, _)| group.clone())
            .collect()
    }

    pub fn lines(&self) -> Vec<TraceLine> {
        lock(&self.spans).lines.clone()
    }

    pub fn records(&self) -> Vec<DiagnosticRecord> {
        lock(&self.diagnostics).clone()
    }

    pub fn group(&self, name: &str) -> Option<TraceGroup> {
        lock(&self.spans)
            .groups
            .iter()
            .find(|(group, _)| group.name == name)
            .map(|(group, _)| group.clone())
    }

    pub fn lines_in(&self, group: &TraceGroup) -> Vec<TraceLine> {
        lock(&self.spans)
            .lines
            .iter()
            .filter(|line| line.group == Some(group.id))
            .cloned()
            .collect()
    }

    /// Messages from trace lines and diagnostic records at error level.
    pub fn errors(&self) -> Vec<String> {
        self.messages(tracing::Level::ERROR, log::Level::Error)
    }

    /// Messages from trace lines and diagnostic records at warning level.
    pub fn warnings(&self) -> Vec<String> {
        self.messages(tracing::Level::WARN, log::Level::Warn)
    }

    pub fn assert_no_errors(&self) {
        let errors = self.errors();
        assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    }

    fn messages(&self, trace_level: tracing::Level, log_level: log::Level) -> Vec<String> {
        lock(&self.spans)
            .lines
            .iter()
            .filter(|line| line.level == trace_level)
            .map(|line| line.message.clone())
            .chain(
                lock(&self.diagnostics)
                    .iter()
                    .filter(|record| record.level == log_level)
                    .map(|record| record.message.clone()),
            )
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct TraceGroup {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
    pub level: tracing::Level,
    pub target: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct TraceLine {
    pub group: Option<u64>,
    pub level: tracing::Level,
    pub target: String,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct DiagnosticRecord {
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

#[derive(Default)]
struct RecordedTrace {
    groups: Vec<(TraceGroup, &'static Metadata<'static>)>,
    lines: Vec<TraceLine>,
    entered: Vec<u64>,
}

impl RecordedTrace {
    fn parent(&self, explicit: Option<&Id>, contextual: bool) -> Option<u64> {
        if let Some(parent) = explicit {
            let id = parent.into_u64();
            return self.group_index(id).map(|_| id);
        }
        if contextual {
            return self.entered.last().copied();
        }
        None
    }

    fn group_index(&self, id: u64) -> Option<usize> {
        self.groups
            .binary_search_by_key(&id, |(group, _)| group.id)
            .ok()
    }
}

#[derive(Clone)]
struct Recorder(Arc<Mutex<RecordedTrace>>);

impl Recorder {
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let metadata = attributes.metadata();
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let mut trace = lock(&self.0);
        let parent = trace.parent(attributes.parent(), attributes.is_contextual());
        let id = next_group_id();
        trace.groups.push((
            TraceGroup {
                id: id.into_u64(),
                parent,
                name: metadata.name().to_owned(),
                level: *metadata.level(),
                target: metadata.target().to_owned(),
                fields: fields.0,
            },
            metadata,
        ));
        id
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut trace = lock(&self.0);
        if let Some(index) = trace.group_index(span.into_u64()) {
            trace.groups[index].0.fields.extend(fields.0);
        }
    }

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        let mut trace = lock(&self.0);
        let group = trace.parent(event.parent(), event.is_contextual());
        trace.lines.push(TraceLine {
            group,
            level: *metadata.level(),
            target: metadata.target().to_owned(),
            message,
            fields: fields.0,
        });
    }

    fn enter(&self, span: &Id) {
        let mut trace = lock(&self.0);
        let id = span.into_u64();
        if trace.group_index(id).is_some() {
            trace.entered.push(id);
        }
    }

    fn current_span(&self) -> Current {
        let trace = lock(&self.0);
        if let Some(&id) = trace.entered.last()
            && let Some(index) = trace.group_index(id)
        {
            return Current::new(Id::from_u64(id), trace.groups[index].1);
        }
        Current::none()
    }

    fn exit(&self, span: &Id) {
        let mut trace = lock(&self.0);
        if let Some(index) = trace.entered.iter().rposition(|&id| id == span.into_u64()) {
            trace.entered.remove(index);
        }
    }
}

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_str(field, &format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

type DiagnosticRecords = Arc<Mutex<Vec<DiagnosticRecord>>>;

struct DiagnosticLogger(Mutex<Option<(ThreadId, DiagnosticRecords)>>);

impl log::Log for DiagnosticLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        lock(&self.0)
            .as_ref()
            .is_some_and(|(owner, _)| *owner == thread::current().id())
    }

    fn log(&self, record: &log::Record<'_>) {
        let records = lock(&self.0)
            .as_ref()
            .filter(|(owner, _)| *owner == thread::current().id())
            .map(|(_, records)| records.clone());
        if let Some(records) = records {
            let message = record.args().to_string();
            lock(&records).push(DiagnosticRecord {
                level: record.level(),
                target: record.target().to_owned(),
                message,
            });
        }
    }

    fn flush(&self) {}
}

struct DiagnosticScope(log::LevelFilter);

impl DiagnosticScope {
    fn new(trace: &Trace) -> Self {
        let previous_level = log::max_level();
        *lock(&LOGGER.0) = Some((thread::current().id(), trace.diagnostics.clone()));
        log::set_max_level(log::LevelFilter::Trace);
        Self(previous_level)
    }
}

impl Drop for DiagnosticScope {
    fn drop(&mut self) {
        *lock(&LOGGER.0) = None;
        log::set_max_level(self.0);
    }
}
