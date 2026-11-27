//! Extension policy for closing a window or quitting the application.

use crate::{Extension, Prompt, shell::CloseRequestHook};
use futures::{
    channel::oneshot,
    future::{Either, select},
};
use gpui::{AnyWindowHandle, App, AsyncApp, Global, Task, Window, WindowId};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    rc::Rc,
};
use tracing::Instrument as _;

const MAX_QUIT_ROUNDS: usize = 8;

/// The operation that requested permission to close a window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseIntent {
    CloseWindow,
    Quit,
}

type PromptAnswer = Box<dyn FnOnce(Option<usize>, &mut Window, &mut App) -> CloseOutcome>;
type Guard = Rc<dyn Fn(CloseIntent, &mut Window, &mut App) -> CloseDecision>;

/// An extension's initial response to a close request.
pub enum CloseDecision {
    Allow,
    Cancel,
    Prompt {
        prompt: Prompt,
        on_answer: PromptAnswer,
    },
    /// Waits until the task resolves. True allows the request to continue.
    Wait(Task<bool>),
}

impl CloseDecision {
    /// Shows a prompt and calls the handler with the chosen button index.
    /// A dismissed prompt cancels the request.
    pub fn prompt(
        prompt: Prompt,
        on_answer: impl FnOnce(Option<usize>, &mut Window, &mut App) -> CloseOutcome + 'static,
    ) -> Self {
        Self::Prompt {
            prompt,
            on_answer: Box::new(on_answer),
        }
    }
}

/// An extension's response to a prompt answer.
pub enum CloseOutcome {
    Allow,
    Cancel,
    /// Waits until the task resolves. True allows the request to continue.
    Wait(Task<bool>),
}

#[derive(Default)]
struct CloseGuards {
    guards: BTreeMap<usize, (Extension, Guard)>,
    closing: BTreeSet<WindowId>,
    quitting: bool,
}

impl Global for CloseGuards {}

#[derive(Default)]
struct EditedWindows(BTreeMap<WindowId, BTreeSet<String>>);

impl Global for EditedWindows {}

impl Extension {
    /// Registers this extension's guard for window close and application quit.
    pub fn on_close_request(
        &self,
        cx: &mut App,
        guard: impl Fn(CloseIntent, &mut Window, &mut App) -> CloseDecision + 'static,
    ) {
        let extension = self.name.as_str();
        let _group = tracing::info_span!("close-guard-register", extension).entered();
        let order = self.registration_order(cx);
        let guards = cx.default_global::<CloseGuards>();
        match guards.guards.entry(order) {
            Entry::Vacant(entry) => {
                entry.insert((self.clone(), Rc::new(guard)));
                tracing::debug!("registered");
            }
            Entry::Occupied(_) => {
                log::error!("extension {extension} already has a close guard");
                return;
            }
        }
        if guards.guards.len() == 1 {
            cx.set_global(CloseRequestHook { close_window, quit });
        }
    }

    /// Marks a window edited on behalf of this extension. The native
    /// indicator on macOS reflects the combined state of all extensions.
    pub fn set_window_edited(&self, window: &mut Window, edited: bool, cx: &mut App) {
        let id = window.window_handle().window_id();
        let extension = self.name.as_str();
        let _group =
            tracing::info_span!("window-edited", window = id.as_u64(), extension, edited).entered();
        let was_edited = is_window_edited(window, cx);
        if edited {
            if !cx.has_global::<EditedWindows>() {
                cx.on_window_closed(|cx, id| {
                    cx.global_mut::<EditedWindows>().0.remove(&id);
                })
                .detach();
            }
            cx.default_global::<EditedWindows>()
                .0
                .entry(id)
                .or_default()
                .insert(self.name.clone());
        } else if was_edited
            && let Entry::Occupied(mut entry) = cx.global_mut::<EditedWindows>().0.entry(id)
        {
            entry.get_mut().remove(extension);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
        let window_edited = is_window_edited(window, cx);
        if window_edited == was_edited {
            tracing::trace!(window_edited, "unchanged");
        } else {
            window.set_window_edited(window_edited);
            tracing::debug!(window_edited, "changed");
        }
    }
}

/// Reads the combined edited state for a window.
pub fn is_window_edited(window: &Window, cx: &App) -> bool {
    let id = window.window_handle().window_id();
    cx.try_global::<EditedWindows>()
        .is_some_and(|windows| windows.0.contains_key(&id))
}

fn window_group(window: AnyWindowHandle, intent: CloseIntent) -> tracing::Span {
    tracing::info_span!(
        "window-close-request",
        window = window.window_id().as_u64(),
        intent = ?intent
    )
}

fn close_window(window: AnyWindowHandle, cx: &mut App) {
    let id = window.window_id();
    let intent = CloseIntent::CloseWindow;
    let group = window_group(window, intent);
    let guards = cx.global_mut::<CloseGuards>();
    if guards.quitting || !guards.closing.insert(id) {
        group.in_scope(|| tracing::info!("already in progress"));
        return;
    }
    group.in_scope(|| tracing::debug!("close requested"));
    let guards: Vec<_> = guards.guards.values().cloned().collect();
    cx.spawn(async move |cx| {
        async move {
            let result = run_guards(window, intent, &guards, cx).await;
            cx.update(|cx| {
                cx.global_mut::<CloseGuards>().closing.remove(&id);
                if result == GuardResult::Allowed
                    && window
                        .update(cx, |_, window, _| {
                            tracing::debug!("removing the window");
                            window.remove_window();
                        })
                        .is_err()
                {
                    tracing::debug!("already closed");
                }
            });
        }
        .instrument(group)
        .await;
    })
    .detach();
}

fn quit(cx: &mut App) {
    let mut windows = cx.windows();
    let group = tracing::info_span!("app-quit", windows = windows.len());
    let guards = cx.global_mut::<CloseGuards>();
    if guards.quitting {
        group.in_scope(|| tracing::info!("already in progress"));
        return;
    }
    if !guards.closing.is_empty() {
        group.in_scope(|| tracing::info!("a close is in progress"));
        return;
    }
    guards.quitting = true;
    let guards: Vec<_> = guards.guards.values().cloned().collect();
    let active = cx.active_window();
    windows.sort_by_key(|&window| Some(window) != active);
    cx.spawn(async move |cx| {
        async move {
            let intent = CloseIntent::Quit;
            let mut checked = BTreeSet::new();
            let mut round = 1;
            let allowed = 'pass: loop {
                tracing::debug!(round, windows = windows.len(), "checking windows");
                for window in windows {
                    checked.insert(window.window_id());
                    let result = run_guards(window, intent, &guards, cx)
                        .instrument(window_group(window, intent))
                        .await;
                    if result == GuardResult::Cancelled {
                        break 'pass false;
                    }
                }
                windows = cx.update(|cx| {
                    let mut windows = cx.windows();
                    windows.retain(|window| !checked.contains(&window.window_id()));
                    let active = cx.active_window();
                    windows.sort_by_key(|&window| Some(window) != active);
                    windows
                });
                if windows.is_empty() {
                    break true;
                }
                if round == MAX_QUIT_ROUNDS {
                    tracing::info!("windows keep opening");
                    break false;
                }
                round += 1;
            };
            cx.update(|cx| {
                cx.global_mut::<CloseGuards>().quitting = false;
                if allowed {
                    tracing::debug!("quitting");
                    cx.quit();
                } else {
                    tracing::info!("cancelled");
                }
            });
        }
        .instrument(group)
        .await;
    })
    .detach();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GuardResult {
    Allowed,
    Cancelled,
    WindowClosed,
}

async fn run_guards(
    window: AnyWindowHandle,
    intent: CloseIntent,
    guards: &[(Extension, Guard)],
    cx: &mut AsyncApp,
) -> GuardResult {
    let result = evaluate_guards(window, intent, guards, cx).await;
    if result == GuardResult::WindowClosed {
        tracing::debug!("already closed");
    }
    result
}

async fn evaluate_guards(
    window: AnyWindowHandle,
    intent: CloseIntent,
    guards: &[(Extension, Guard)],
    cx: &mut AsyncApp,
) -> GuardResult {
    for (owner, guard) in guards {
        let extension = owner.name.as_str();
        let Ok(decision) = window.update(cx, |_, window, cx| guard(intent, window, cx)) else {
            return GuardResult::WindowClosed;
        };
        let outcome = match decision {
            CloseDecision::Allow => CloseOutcome::Allow,
            CloseDecision::Cancel => CloseOutcome::Cancel,
            CloseDecision::Wait(task) => CloseOutcome::Wait(task),
            CloseDecision::Prompt { prompt, on_answer } => {
                tracing::debug!(extension, "prompted");
                answer_prompt(window, owner, prompt, on_answer, cx).await
            }
        };
        let allowed = match outcome {
            CloseOutcome::Allow => true,
            CloseOutcome::Cancel => false,
            CloseOutcome::Wait(task) => {
                tracing::debug!(extension, "waiting");
                task.await
            }
        };
        if !cx.update(|cx| cx.windows().contains(&window)) {
            return GuardResult::WindowClosed;
        }
        if !allowed {
            tracing::debug!(extension, "cancelled");
            tracing::info!(extension, "cancelled");
            return GuardResult::Cancelled;
        }
        tracing::debug!(extension, "allowed");
    }
    GuardResult::Allowed
}

async fn answer_prompt(
    handle: AnyWindowHandle,
    owner: &Extension,
    prompt: Prompt,
    on_answer: PromptAnswer,
    cx: &mut AsyncApp,
) -> CloseOutcome {
    let (sender, receiver) = oneshot::channel();
    let (closed, closed_signal) = oneshot::channel();
    let mut closed = Some(closed);
    let id = handle.window_id();
    let Ok(_subscription) = handle.update(cx, |_, window, cx| {
        let subscription = cx.on_window_closed(move |_, closed_id| {
            if closed_id == id
                && let Some(closed) = closed.take()
            {
                let _ = closed.send(());
            }
        });
        window.activate_window();
        owner.prompt(prompt, window, cx, move |answer, _| {
            let _ = sender.send(answer);
        });
        subscription
    }) else {
        return CloseOutcome::Cancel;
    };
    if let Either::Right((Ok(Some(answer)), _)) = select(closed_signal, receiver).await
        && let Ok(outcome) = handle.update(cx, |_, window, cx| on_answer(Some(answer), window, cx))
    {
        return outcome;
    }
    CloseOutcome::Cancel
}
