//! Adapts Zed's `crates/workspace/src/toast_layer.rs` and
//! `crates/notifications/src/status_toast.rs` at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    App, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};
use ui::{animation::DefaultAnimations, prelude::*};

const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(10);
const MINIMUM_RESUME_DURATION: Duration = Duration::from_millis(800);

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// The severity of a toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warning,
    Error,
}
impl ToastKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
    fn icon(self) -> (IconName, Color) {
        match self {
            Self::Info => (IconName::Info, Color::Muted),
            Self::Warning => (IconName::Warning, Color::Warning),
            Self::Error => (IconName::XCircle, Color::Error),
        }
    }
}

/// What a toast shows.
pub struct Toast {
    kind: ToastKind,
    text: SharedString,
    action: Option<(SharedString, Handler)>,
}
impl Toast {
    pub fn new(kind: ToastKind, text: impl Into<SharedString>) -> Self {
        Self {
            kind,
            text: text.into(),
            action: None,
        }
    }
    /// Sets the toast's one button, replacing any previous button.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(on_click)));
        self
    }
}

/// Why a toast was closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastHideReason {
    Timeout,
    DismissButton,
    Action,
    MiddleClick,
    HideToast,
    Replaced,
}
impl ToastHideReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::DismissButton => "dismiss-button",
            Self::Action => "action",
            Self::MiddleClick => "middle-click",
            Self::HideToast => "hide-toast",
            Self::Replaced => "replaced",
        }
    }
}
struct DismissTimer {
    instant_started: Instant,
    _task: Task<()>,
}

/// A window's transient message layer.
pub struct ToastLayer {
    active_toast: Option<Toast>,
    id: usize,
    duration_remaining: Duration,
    dismiss_timer: Option<DismissTimer>,
}
impl Default for ToastLayer {
    fn default() -> Self {
        Self::new()
    }
}
impl ToastLayer {
    pub fn new() -> Self {
        Self {
            active_toast: None,
            id: 0,
            duration_remaining: DEFAULT_TOAST_DURATION,
            dismiss_timer: None,
        }
    }
    pub fn show_toast(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.hide_toast(ToastHideReason::Replaced, cx);
        let _group = tracing::info_span!("toast-show", kind = toast.kind.as_str()).entered();
        self.id += 1;
        self.active_toast = Some(toast);
        self.start_dismiss_timer(DEFAULT_TOAST_DURATION, cx);
        tracing::debug!(
            duration_ms = u64::try_from(DEFAULT_TOAST_DURATION.as_millis()).unwrap_or(u64::MAX),
            "timer started"
        );
        cx.notify();
    }
    pub fn hide_toast(&mut self, reason: ToastHideReason, cx: &mut Context<Self>) {
        let Some(toast) = self.active_toast.take() else {
            return;
        };
        let _group = tracing::info_span!(
            "toast-hide",
            kind = toast.kind.as_str(),
            reason = reason.as_str()
        )
        .entered();
        self.dismiss_timer.take();
        cx.notify();
    }
    pub fn has_active_toast(&self) -> bool {
        self.active_toast.is_some()
    }
    fn start_dismiss_timer(&mut self, duration: Duration, cx: &mut Context<Self>) {
        self.dismiss_timer.take();
        let instant_started = cx.background_executor().now();
        let id = self.id;
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                if this.id == id {
                    this.hide_toast(ToastHideReason::Timeout, cx);
                }
            });
        });
        self.duration_remaining = duration;
        self.dismiss_timer = Some(DismissTimer {
            instant_started,
            _task: task,
        });
    }
    fn pause_dismiss_timer(&mut self, cx: &mut Context<Self>) {
        let Some(timer) = self.dismiss_timer.take() else {
            return;
        };
        self.duration_remaining = self.duration_remaining.saturating_sub(
            cx.background_executor()
                .now()
                .duration_since(timer.instant_started),
        );
        tracing::debug!(
            remaining_ms = u64::try_from(self.duration_remaining.as_millis()).unwrap_or(u64::MAX),
            "timer paused"
        );
    }
    fn restart_dismiss_timer(&mut self, cx: &mut Context<Self>) {
        if self.active_toast.is_none() || self.dismiss_timer.is_some() {
            return;
        }
        let duration = self.duration_remaining.max(MINIMUM_RESUME_DURATION);
        self.start_dismiss_timer(duration, cx);
        tracing::debug!(
            remaining_ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            "timer resumed"
        );
    }
}
impl Render for ToastLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(toast) = &self.active_toast else {
            return div().into_any_element();
        };
        let kind = toast.kind;
        tracing::trace!(kind = kind.as_str(), "rendering toast layer");
        let (icon, color) = kind.icon();
        let id = self.id;
        let mut row = h_flex()
            .id("toast")
            .debug_selector(|| "TOAST".into())
            .elevation_3(cx)
            .gap_2()
            .py_1p5()
            .pl_2p5()
            .pr_1p5()
            .flex_none()
            .bg(cx.theme().colors().surface_background)
            .shadow_lg()
            .child(Icon::new(icon).size(IconSize::Small).color(color))
            .child(Label::new(toast.text.clone()).color(Color::Default));
        if let Some((label, handler)) = &toast.action {
            let label = label.clone();
            let handler = handler.clone();
            let layer = cx.entity().downgrade();
            row = row.child(
                div().debug_selector(|| "TOAST-ACTION".into()).child(
                    Button::new("toast-action", label.clone())
                        .color(Color::Muted)
                        .on_click(move |_, window, cx| {
                            let active = layer
                                .read_with(cx, |this, _| {
                                    this.id == id && this.active_toast.is_some()
                                })
                                .unwrap_or(false);
                            if !active {
                                return;
                            }
                            {
                                let _group = tracing::info_span!(
                                    "toast-action",
                                    kind = kind.as_str(),
                                    label = label.as_ref()
                                )
                                .entered();
                                handler(window, cx);
                            }
                            let _ = layer.update(cx, |this, cx| {
                                if this.id == id {
                                    this.hide_toast(ToastHideReason::Action, cx);
                                }
                            });
                        }),
                ),
            );
        }
        row = row.child(
            div().debug_selector(|| "TOAST-DISMISS".into()).child(
                IconButton::new("toast-dismiss", IconName::Close)
                    .shape(ui::IconButtonShape::Square)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.hide_toast(ToastHideReason::DismissButton, cx)
                    })),
            ),
        );
        div()
            .absolute()
            .size_full()
            .bottom_0()
            .left_0()
            .child(
                v_flex()
                    .id(("toast-layer-container", id))
                    .absolute()
                    .w_full()
                    .bottom(px(0.))
                    .items_center()
                    .child(
                        h_flex()
                            .id("active-toast-container")
                            .occlude()
                            .on_hover(cx.listener(|this, hovering, _, cx| {
                                if *hovering {
                                    this.pause_dismiss_timer(cx);
                                } else {
                                    this.restart_dismiss_timer(cx);
                                }
                                cx.stop_propagation();
                            }))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .on_mouse_down(
                                MouseButton::Middle,
                                cx.listener(|this, _, _, cx| {
                                    this.hide_toast(ToastHideReason::MiddleClick, cx)
                                }),
                            )
                            .child(row),
                    )
                    .animate_in(AnimationDirection::FromBottom, true),
            )
            .into_any_element()
    }
}
