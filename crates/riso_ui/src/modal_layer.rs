//! Adapts Zed's `workspace` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use crate::{h_flex, v_flex};
use gpui::{
    AnyView, App, AppContext, Context, DismissEvent, Entity, FocusHandle, InteractiveElement,
    IntoElement, ManagedView, MouseButton, ParentElement, Render, Styled, Subscription, Window,
    div, prelude::FluentBuilder, px,
};
use theme::ActiveTheme;

/// Reason a modal was closed by or through [`ModalLayer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalHideReason {
    DismissEvent,
    Escape,
    ClickOutside,
    HideModal,
    Toggle,
    Replaced,
}

impl ModalHideReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::DismissEvent => "dismiss-event",
            Self::Escape => "escape",
            Self::ClickOutside => "click-outside",
            Self::HideModal => "hide-modal",
            Self::Toggle => "toggle",
            Self::Replaced => "replaced",
        }
    }
}

/// A view that can be shown inside [`ModalLayer`].
pub trait ModalView: ManagedView {
    fn dims_background(&self) -> bool {
        false
    }
}

trait ModalViewHandle {
    fn view(&self) -> AnyView;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn subscribe_dismiss(&self, window: &mut Window, cx: &mut Context<ModalLayer>) -> Subscription;
    fn dims_background(&self, cx: &App) -> bool;
}

impl<V: ModalView> ModalViewHandle for Entity<V> {
    fn view(&self) -> AnyView {
        self.clone().into()
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }

    fn subscribe_dismiss(&self, window: &mut Window, cx: &mut Context<ModalLayer>) -> Subscription {
        cx.subscribe_in(self, window, |this, _, _: &DismissEvent, window, cx| {
            this.hide_modal(ModalHideReason::DismissEvent, window, cx);
        })
    }

    fn dims_background(&self, cx: &App) -> bool {
        self.read(cx).dims_background()
    }
}

struct ActiveModal {
    name: &'static str,
    modal: Box<dyn ModalViewHandle>,
    _subscription: Subscription,
    previous_focus_handle: Option<FocusHandle>,
    focus_handle: FocusHandle,
}

/// A window layer that shows at most one modal view at a time.
pub struct ModalLayer {
    active_modal: Option<ActiveModal>,
}

impl Default for ModalLayer {
    fn default() -> Self {
        Self::new()
    }
}

fn short_type_name<V>() -> &'static str {
    let full = std::any::type_name::<V>();
    full.rsplit("::").next().unwrap_or(full)
}

impl ModalLayer {
    pub fn new() -> Self {
        Self { active_modal: None }
    }

    /// Toggles a modal of type `V`: closes it if the same type is open, replaces a
    /// different open modal, or shows a new one when none is open.
    pub fn toggle_modal<V, B>(&mut self, window: &mut Window, cx: &mut Context<Self>, build: B)
    where
        V: ModalView,
        B: FnOnce(&mut Window, &mut Context<V>) -> V,
    {
        let mut previous_focus_handle = window.focused(cx);
        if let Some(active_modal) = &self.active_modal {
            let should_close = active_modal.modal.view().downcast::<V>().is_ok();
            previous_focus_handle = active_modal.previous_focus_handle.clone();
            let reason = if should_close {
                ModalHideReason::Toggle
            } else {
                ModalHideReason::Replaced
            };
            self.hide_modal(reason, window, cx);
            if should_close {
                return;
            }
        }
        let name = short_type_name::<V>();
        let _group = tracing::info_span!("modal-show", modal = name).entered();
        let new_modal = cx.new(|cx| build(window, cx));
        self.show_modal(Box::new(new_modal), name, previous_focus_handle, window, cx);
    }

    fn show_modal(
        &mut self,
        modal: Box<dyn ModalViewHandle>,
        name: &'static str,
        previous_focus_handle: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus_handle = cx.focus_handle();
        let modal_focus_handle = modal.focus_handle(cx);
        let dismiss_subscription = modal.subscribe_dismiss(window, cx);
        self.active_modal = Some(ActiveModal {
            name,
            modal,
            _subscription: dismiss_subscription,
            previous_focus_handle,
            focus_handle,
        });
        cx.defer_in(window, move |_, window, cx| {
            window.focus(&modal_focus_handle, cx);
        });
        cx.notify();
    }

    /// Closes the currently active modal, if one is open, restoring prior focus.
    pub fn hide_modal(
        &mut self,
        reason: ModalHideReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_modal) = self.active_modal.take() else {
            return;
        };
        let _group = tracing::info_span!(
            "modal-hide",
            modal = active_modal.name,
            reason = reason.as_str(),
        )
        .entered();
        if let Some(previous_focus) = active_modal.previous_focus_handle
            && active_modal.focus_handle.contains_focused(window, cx)
        {
            previous_focus.focus(window, cx);
            tracing::debug!("focus restored");
        }
        cx.notify();
    }

    pub fn has_active_modal(&self) -> bool {
        self.active_modal.is_some()
    }
}

impl Render for ModalLayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(active_modal) = &self.active_modal else {
            return div().into_any_element();
        };
        let dimmed = active_modal.modal.dims_background(cx);
        tracing::trace!(modal = active_modal.name, dimmed, "rendering modal layer",);

        div()
            .absolute()
            .size_full()
            .inset_0()
            .occlude()
            .when(dimmed, |this| {
                let mut background = cx.theme().colors().elevated_surface_background;
                background.fade_out(0.2);
                this.bg(background)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.hide_modal(ModalHideReason::ClickOutside, window, cx);
                }),
            )
            .child(
                v_flex()
                    .h(px(0.0))
                    .top_20()
                    .items_center()
                    .key_context("ModalLayer")
                    .track_focus(&active_modal.focus_handle)
                    .child(
                        h_flex()
                            .occlude()
                            .child(active_modal.modal.view())
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            }),
                    ),
            )
            .into_any_element()
    }
}
