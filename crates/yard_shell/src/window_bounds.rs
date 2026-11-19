//! The main window's place, size, and screen, saved when they change and
//! used at the next start, as Zed's workspace saves its window bounds.

use gpui::{
    App, AppContext as _, Bounds, Context, DisplayId, Task, Window, WindowBounds, point, px, size,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tracing::Instrument as _;
use yard_paths::write_atomic;

/// The version of the file's format.
const VERSION: u64 = 1;

/// How long a save waits after a change, so that a drag or a resize writes
/// once at its end.
const SAVE_DELAY: Duration = Duration::from_millis(100);

#[derive(Serialize, Deserialize)]
struct SavedWindow {
    version: u64,
    display: String,
    bounds: SavedBounds,
}

#[derive(Serialize, Deserialize)]
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

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Windowed => "windowed",
            State::Maximized => "maximized",
            State::Fullscreen => "fullscreen",
        }
    }
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

/// The saved bounds and screen for the next window, or `None` for the default.
pub fn saved_window_bounds(cx: &App, file: &Path) -> Option<(WindowBounds, DisplayId)> {
    let _group = tracing::info_span!("window-bounds-restore", file = %file.display()).entered();
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!("no saved window bounds");
            return None;
        }
        Err(error) => {
            log::warn!("the saved window bounds cannot be read: {error}");
            return None;
        }
    };
    let saved = match decode(&text) {
        Ok(saved) => saved,
        Err(reason) => {
            log::warn!("the saved window bounds cannot be read: {reason}");
            return None;
        }
    };
    let display = cx.displays().into_iter().find(|display| {
        display
            .uuid()
            .is_ok_and(|uuid| uuid.to_string().eq_ignore_ascii_case(&saved.display))
    });
    let Some(display) = display else {
        tracing::info!(
            display = saved.display.as_str(),
            "no screen has the saved UUID; the window opens at the default place"
        );
        return None;
    };
    tracing::debug!(
        state = saved.bounds.state.name(),
        display = saved.display.as_str(),
        "restored"
    );
    Some(((&saved.bounds).into(), display.id()))
}

fn decode(text: &str) -> Result<SavedWindow, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(VERSION) => {}
        Some(version) => return Err(format!("version {version}, not {VERSION}")),
        None => return Err(String::from("no version")),
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Saves `window`'s bounds to `file` when they change, as described above.
pub fn track_window_bounds(window: &mut Window, cx: &mut App, file: PathBuf) {
    let tracker = cx.new(|cx| {
        // The window is gone: write the save that waits, with the last
        // bounds it saw.
        cx.on_release(|tracker: &mut BoundsTracker, cx| {
            if let Some(write) = tracker.write_pending(cx) {
                write.detach();
            }
        })
        .detach();
        // The application quits: write the save that waits, and keep the
        // process until it is written. The window's release then finds no
        // save waiting, so the file is not written twice.
        cx.on_app_quit(|tracker: &mut BoundsTracker, cx| {
            let write = tracker.write_pending(cx);
            async move {
                if let Some(write) = write {
                    write.await;
                }
            }
        })
        .detach();
        BoundsTracker {
            file,
            pending: None,
            latest: None,
            sequence: 0,
            written: Arc::default(),
        }
    });
    let keep_alive = tracker.clone();
    tracker.update(cx, |_, cx| {
        cx.observe_window_bounds(window, move |tracker, window, cx| {
            // The observer lives as long as the window, and keeps the
            // tracker, with its pending save, alive as long.
            let _ = &keep_alive;
            tracker.bounds_changed(window, cx);
        })
        .detach();
    });
}

struct BoundsTracker {
    file: PathBuf,
    /// The save that waits, as Zed's `bounds_save_task_queued`.
    pending: Option<Task<()>>,
    /// The window's bounds and screen UUID at its last change while active.
    latest: Option<(SavedBounds, Option<String>)>,
    /// The number of the last write started.
    sequence: u64,
    /// The number of the last write done, so that an older write never
    /// replaces a newer one.
    written: Arc<Mutex<u64>>,
}

impl BoundsTracker {
    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        if !active {
            tracing::trace!(active, "window bounds changed");
            return;
        }
        let display = window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string());
        self.latest = Some((window.inner_window_bounds().into(), display));
        if self.pending.is_some() {
            tracing::trace!(active, "window bounds changed");
            return;
        }
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                this.pending.take();
                if let Some(write) = this.write(cx) {
                    write.detach();
                }
            })
            .ok();
        }));
    }

    /// Writes the save that waits now, if one waits.
    fn write_pending(&mut self, cx: &App) -> Option<Task<()>> {
        // Dropping the waiting task cancels its timer.
        drop(self.pending.take()?);
        self.write(cx)
    }

    /// Writes the last bounds on the background executor, inside the save's
    /// group.
    fn write(&mut self, cx: &App) -> Option<Task<()>> {
        let (bounds, display) = self.latest.take()?;
        let group = tracing::info_span!("window-bounds-save", state = bounds.state.name());
        let Some(display) = display else {
            group.in_scope(|| tracing::debug!("not saved: the window's screen has no UUID"));
            return None;
        };
        let saved = SavedWindow {
            version: VERSION,
            display,
            bounds,
        };
        let bytes = serde_json::to_vec_pretty(&saved).expect("window bounds encode as JSON");
        self.sequence += 1;
        let sequence = self.sequence;
        let written = self.written.clone();
        let file = self.file.clone();
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
}
