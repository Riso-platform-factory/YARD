//! `cargo run dev`: no tour unless a `LOG_TOUR` case is requested.
//! `LOG_TOUR=levels cargo run dev`: nested groups, WARN success, INFO refusal, ERROR failure, async steps, and a window opened then closed.
//! `LOG_TOUR=panic cargo run dev`: main-thread panic fails open groups and the phase, flushes both logs, and writes run-end before abort.
//! `LOG_TOUR=panic-thread cargo run dev`: background-thread panic fails its group; the app keeps running.
//! `LOG_TOUR=interrupt cargo run dev`: exit leaves open groups interrupted (⊘) and writes run-end.
//! `LOG_TOUR=levels RISO_LIFECYCLE=trace cargo run dev`: shows the tour's debug and trace steps; the default only raises crates under review.
//! `RISO_LIFECYCLE='info,[' cargo run dev`: invalid filter WARN and fallback to info.
//! Rotation is covered by crates/riso_log/tests/rotation.rs.

use gpui::AppContext;
use tracing::Instrument;

pub fn init(cx: &mut gpui::App) {
    let case = std::env::var("LOG_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("log-tour").entered();
    match case.as_str() {
        "levels" => {}
        "panic" => {
            let _group = tracing::info_span!("panic-work").entered();
            panic!("tour panic: deliberate failure inside an open group");
        }
        "panic-thread" => {
            std::thread::spawn(|| {
                let _group = tracing::info_span!("panic-thread-work").entered();
                panic!("tour panic: deliberate background failure inside an open group");
            });
            return;
        }
        "interrupt" => {
            let _group = tracing::info_span!("interrupted-work").entered();
            tracing::info!("exiting without dropping this group");
            std::process::exit(0);
        }
        _ => {
            log::warn!(
                "unknown LOG_TOUR case {case:?}; known cases: levels, panic, panic-thread, interrupt"
            );
            return;
        }
    }
    let workspace = tempfile::tempdir().expect("create tour workspace");
    let folder = workspace.path().to_path_buf();
    std::fs::write(folder.join("sample.txt"), "tour document").expect("write sample document");
    {
        let _group = tracing::info_span!("load-workspace", workspace = %folder.display()).entered();
        tracing::debug!("reading workspace entries");
        let _child = tracing::info_span!("inspect-files").entered();
        let count = std::fs::read_dir(&folder).unwrap().count();
        tracing::debug!(count, "counted workspace files");
        for entry in std::fs::read_dir(&folder).unwrap() {
            tracing::trace!(path = %entry.unwrap().path().display(), "inspected file");
        }
        tracing::info!("workspace ready\ncontinuation line; escaped control: \u{1b}");
    }
    {
        let _group = tracing::info_span!("load-preferences").entered();
        match std::fs::read_to_string(folder.join("log-tour-missing/preferences.json")) {
            Ok(_) => tracing::debug!("loaded preferences"),
            Err(error) => log::warn!("preferences unavailable: {error}; using defaults"),
        }
    }
    {
        let _group = tracing::info_span!("validate-selection", selected = 0).entered();
        tracing::info!("export refused: select at least one item");
    }
    {
        let _group = tracing::info_span!("load-document").entered();
        if let Err(error) = std::fs::read(folder.join("log-tour-missing/document")) {
            log::error!("document load failed: {error}");
        }
    }
    let window = {
        let _group = tracing::info_span!("open-window").entered();
        cx.open_window(gpui::WindowOptions::default(), |_, cx| {
            cx.new(|_| gpui::Empty)
        })
        .expect("open tour window")
    };
    let span = tracing::info_span!("scan-background", folder = %folder.display());
    let task = cx.background_executor().spawn(
        async move {
            let _workspace = workspace;
            tracing::debug!("background scan started");
            let count = std::fs::read_dir(&folder).expect("scan workspace").count();
            tracing::trace!(count, "background scan finished");
        }
        .instrument(span),
    );
    cx.spawn(async move |cx| {
        task.await;
        let _group = tracing::info_span!("close-window").entered();
        window
            .update(cx, |_, window, _| window.remove_window())
            .expect("close tour window");
        tracing::debug!("closed tour window");
    })
    .detach();
}
