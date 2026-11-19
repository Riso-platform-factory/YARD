//! The windows open at quit, saved as they change and opened again at the
//! next start, as Zed's session reopens its windows.

use gpui::{
    AnyWindowHandle, App, Bounds, DisplayId, Global, Task, Window, WindowBounds, WindowId, point,
    px, size,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tracing::Instrument as _;
use yard_layout::{Operation, decode, encode};
use yard_paths::write_atomic;

/// The version of the file's format.
const VERSION: u64 = 1;

/// How long a save waits after a change, so that a drag, a resize, or a run
/// of operations writes once at its end.
const SAVE_DELAY: Duration = Duration::from_millis(100);

/// One window of the saved session: what the startup needs to open it.
#[derive(Clone, Debug)]
pub struct SessionWindow {
    /// The window's bounds, or `None` when no screen has the saved UUID.
    pub bounds: Option<WindowBounds>,
    /// The screen with the saved UUID, or `None` for the default place.
    pub display: Option<DisplayId>,
    /// The named layout the window showed.
    pub layout: String,
    /// The window's user layer for each named layout it has shown.
    pub layers: BTreeMap<String, Vec<Operation>>,
    /// The session file the window was read from.
    pub(super) file: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct SavedSession {
    version: u64,
    windows: Vec<SavedWindow>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedWindow {
    bounds: SavedBounds,
    display: Option<String>,
    layout: String,
    layers: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedBounds {
    state: State,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Windowed,
    Maximized,
    Fullscreen,
}

impl From<WindowBounds> for SavedBounds {
    fn from(bounds: WindowBounds) -> Self {
        let (state, bounds) = match bounds {
            WindowBounds::Windowed(bounds) => (State::Windowed, bounds),
            WindowBounds::Maximized(bounds) => (State::Maximized, bounds),
            WindowBounds::Fullscreen(bounds) => (State::Fullscreen, bounds),
        };
        SavedBounds {
            state,
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
        }
    }
}

impl From<&SavedBounds> for WindowBounds {
    fn from(saved: &SavedBounds) -> Self {
        let bounds = Bounds::new(
            point(px(saved.x), px(saved.y)),
            size(px(saved.width), px(saved.height)),
        );
        match saved.state {
            State::Windowed => WindowBounds::Windowed(bounds),
            State::Maximized => WindowBounds::Maximized(bounds),
            State::Fullscreen => WindowBounds::Fullscreen(bounds),
        }
    }
}

/// The windows of the saved session, or `None` to start as before.
pub fn saved_session(cx: &App, file: &Path) -> Option<Vec<SessionWindow>> {
    let _group = tracing::info_span!("session-restore", file = %file.display()).entered();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!("no saved session");
            return None;
        }
        Err(error) => {
            log::warn!("the saved session cannot be read: {error}");
            return None;
        }
    };
    let saved = match decode_session(&text) {
        Ok(saved) => saved,
        Err(reason) => {
            log::warn!("the saved session cannot be read: {reason}");
            return None;
        }
    };
    if saved.is_empty() {
        tracing::debug!("no saved session");
        return None;
    }
    let displays = cx.displays();
    let windows = saved
        .into_iter()
        .map(|(window, layers)| {
            let display = window.display.as_ref().and_then(|saved| {
                let display = displays.iter().find(|display| {
                    display
                        .uuid()
                        .is_ok_and(|uuid| uuid.to_string().eq_ignore_ascii_case(saved))
                });
                if display.is_none() {
                    tracing::info!(
                        display = saved.as_str(),
                        "no screen has the saved UUID; the window opens at the default place"
                    );
                }
                display.map(|display| display.id())
            });
            SessionWindow {
                bounds: display.map(|_| (&window.bounds).into()),
                display,
                layout: window.layout,
                layers,
                file: file.to_path_buf(),
            }
        })
        .collect::<Vec<_>>();
    tracing::debug!(windows = windows.len(), "restored");
    Some(windows)
}

type DecodedWindow = (SavedWindow, BTreeMap<String, Vec<Operation>>);

fn decode_session(text: &str) -> Result<Vec<DecodedWindow>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(VERSION) => {}
        Some(version) => return Err(format!("version {version}, not {VERSION}")),
        None => return Err(String::from("no version")),
    }
    let saved: SavedSession = serde_json::from_value(value).map_err(|error| error.to_string())?;
    saved
        .windows
        .into_iter()
        .map(|mut window| {
            let layers = std::mem::take(&mut window.layers)
                .into_iter()
                .map(|(layout, operations)| {
                    decode_layer(operations)
                        .map(|operations| (layout.clone(), operations))
                        .map_err(|error| format!("layer {layout}: {error}"))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            Ok((window, layers))
        })
        .collect()
}

/// A layer is the list of operations of the saved layout format.
fn encode_layer(operations: &[Operation]) -> serde_json::Value {
    let mut file: serde_json::Value =
        serde_json::from_str(&encode(operations)).expect("a layer encodes as JSON");
    file["operations"].take()
}

fn decode_layer(operations: serde_json::Value) -> Result<Vec<Operation>, String> {
    let mut file: serde_json::Value =
        serde_json::from_str(&encode(&[])).expect("a layer encodes as JSON");
    file["operations"] = operations;
    decode(&file.to_string()).map_err(|error| error.to_string())
}

#[derive(Default)]
struct Session(RefCell<SessionState>);

impl Global for Session {}

#[derive(Default)]
struct SessionState {
    /// The file the session is kept in, once `track_session` names it.
    file: Option<PathBuf>,
    /// Every open shell window, in the order it opened.
    windows: Vec<AnyWindowHandle>,
    /// Each open window's bounds and screen UUID at its last change, for
    /// the record of the last window after it closes.
    bounds: BTreeMap<WindowId, (WindowBounds, Option<String>)>,
    /// The last window, closed: closing it quits the application, so it
    /// stays in the session.
    kept: Option<SavedWindow>,
    /// The windows already logged as left out of a save.
    warned: BTreeSet<WindowId>,
    /// The save that waits.
    pending: Option<Task<()>>,
    /// The application quits: its windows close without leaving the session.
    quitting: bool,
    /// The quit flush is registered.
    flush_on_quit: bool,
    /// The number of the last write started.
    sequence: u64,
    /// The number of the last write done, so that an older write never
    /// replaces a newer one.
    written: Arc<Mutex<u64>>,
}

/// Keeps `file` up to date with every open window, as described above.
pub fn track_session(cx: &mut App, file: PathBuf) {
    tracing::debug!(file = %file.display(), "session file set");
    let register_quit = {
        let mut state = cx.default_global::<Session>().0.borrow_mut();
        state.file = Some(file);
        !std::mem::replace(&mut state.flush_on_quit, true)
    };
    if register_quit {
        // The application quits: save every window still open, once, before
        // the windows close, and keep the process until it is written.
        cx.on_app_quit(|cx| {
            if let Some(session) = cx.try_global::<Session>() {
                let mut state = session.0.borrow_mut();
                state.quitting = true;
                // Dropping the waiting save cancels its timer.
                state.pending.take();
            }
            let flush = write(cx);
            async move {
                if let Some(flush) = flush {
                    flush.await;
                }
            }
        })
        .detach();
    }
    queue_save(cx);
}

/// The window's bounds and the UUID of its screen.
fn placement(window: &Window, cx: &App) -> (WindowBounds, Option<String>) {
    let display = window
        .display(cx)
        .and_then(|display| display.uuid().ok())
        .map(|uuid| uuid.to_string());
    (window.inner_window_bounds(), display)
}

/// A shell window opened: it joins the session.
pub(super) fn window_opened(window: &Window, cx: &mut App) {
    let placement = placement(window, cx);
    {
        let mut state = cx.default_global::<Session>().0.borrow_mut();
        let handle = window.window_handle();
        state.windows.push(handle);
        state.bounds.insert(handle.window_id(), placement);
        state.kept = None;
    }
    queue_save(cx);
}

/// A shell window's bounds changed.
pub(super) fn bounds_changed(window: &Window, cx: &App) {
    let placement = placement(window, cx);
    if let Some(session) = cx.try_global::<Session>() {
        session
            .0
            .borrow_mut()
            .bounds
            .insert(window.window_handle().window_id(), placement);
    }
    queue_save(cx);
}

/// A shell window closed during the run: it leaves the session, except
/// the last one, whose closing quits the application.
pub(super) fn window_closed(cx: &mut App, id: WindowId) {
    let placement = {
        let Some(session) = cx.try_global::<Session>() else {
            return;
        };
        let mut state = session.0.borrow_mut();
        if state.quitting {
            return;
        }
        let last = state.windows.iter().all(|window| window.window_id() == id);
        state.windows.retain(|window| window.window_id() != id);
        let placement = state.bounds.remove(&id);
        placement.filter(|_| last)
    };
    if let Some((bounds, display)) = placement
        && let Some((layout, layers)) = super::shell::session_layers(cx, id)
    {
        tracing::debug!(window = id.as_u64(), "last window kept in the session");
        cx.global::<Session>().0.borrow_mut().kept =
            Some(saved_window(bounds, display, layout, &layers));
    }
    queue_save(cx);
}

fn saved_window(
    bounds: WindowBounds,
    display: Option<String>,
    layout: String,
    layers: &BTreeMap<String, Vec<Operation>>,
) -> SavedWindow {
    SavedWindow {
        bounds: bounds.into(),
        display,
        layout,
        layers: layers
            .iter()
            .map(|(layout, operations)| (layout.clone(), encode_layer(operations)))
            .collect(),
    }
}

/// A window's bounds or layout changed.
pub(super) fn layout_changed(cx: &App) {
    queue_save(cx);
}

/// Queues a save that writes 100 ms later; a change while a save waits
/// joins it.
fn queue_save(cx: &App) {
    let Some(session) = cx.try_global::<Session>() else {
        return;
    };
    let mut state = session.0.borrow_mut();
    if state.file.is_none() || state.quitting {
        return;
    }
    if state.pending.is_some() {
        tracing::trace!("session changed");
        return;
    }
    state.pending = Some(cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        cx.update(|cx| {
            let pending = cx
                .try_global::<Session>()
                .and_then(|session| session.0.borrow_mut().pending.take());
            // This task is the one that waited: let it finish.
            if let Some(pending) = pending {
                pending.detach();
            }
            if let Some(write) = write(cx) {
                write.detach();
            }
        });
    }));
}

/// A window the session cannot read is left out of the save: logged once
/// in the diagnostic log, and in the trace every time.
fn left_out(cx: &App, id: WindowId, reason: &str) {
    let first = cx.global::<Session>().0.borrow_mut().warned.insert(id);
    if first {
        log::warn!(
            "window {} is left out of the session: {reason}",
            id.as_u64()
        );
    } else {
        tracing::warn!(window = id.as_u64(), reason, "left out of the session");
    }
}

/// Writes every open window on the background executor, inside the save's
/// group.
fn write(cx: &mut App) -> Option<Task<()>> {
    let (file, handles, kept) = {
        let state = cx.try_global::<Session>()?.0.borrow();
        (
            state.file.clone()?,
            state.windows.clone(),
            state.kept.clone(),
        )
    };
    let mut windows = Vec::with_capacity(handles.len() + 1);
    for handle in handles {
        let id = handle.window_id();
        let window = handle.update(cx, |_, window, cx| {
            let (bounds, display) = placement(window, cx);
            super::shell::session_layers(cx, id)
                .map(|(layout, layers)| saved_window(bounds, display, layout, &layers))
                .ok_or("the window has no shell layout")
        });
        match window {
            Ok(Ok(window)) => windows.push(window),
            Ok(Err(reason)) => left_out(cx, id, reason),
            Err(error) => left_out(cx, id, &error.to_string()),
        }
    }
    windows.extend(kept);
    let group = tracing::info_span!("session-save", windows = windows.len());
    let bytes = serde_json::to_vec_pretty(&SavedSession {
        version: VERSION,
        windows,
    })
    .expect("the session encodes as JSON");
    let (sequence, written) = {
        let mut state = cx.global::<Session>().0.borrow_mut();
        state.sequence += 1;
        (state.sequence, state.written.clone())
    };
    Some(
        cx.background_executor().spawn(
            async move {
                let mut written = written.lock().unwrap_or_else(PoisonError::into_inner);
                if *written > sequence {
                    tracing::debug!("skipped: a newer save is written");
                    return;
                }
                if write_atomic(&file, &bytes).is_ok() {
                    tracing::debug!("saved");
                }
                *written = sequence;
            }
            .instrument(group),
        ),
    )
}
