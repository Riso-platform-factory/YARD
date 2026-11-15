//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    Action, AnyView, App, AppContext, Context, FocusHandle, IntoElement, Render, SharedString,
    Window,
};

/// A tooltip in Zed's layout, with optional action keys and detail.
pub struct Tooltip {
    inner: gpui::Entity<ui::Tooltip>,
}

impl Tooltip {
    pub fn text(title: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        Self::builder(title.into(), None, None, None)
    }

    pub fn for_action<T: Into<SharedString>>(
        title: T,
        action: &dyn Action,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + use<T> {
        Self::builder(title.into(), Some(action.boxed_clone()), None, None)
    }

    pub fn for_action_in<T: Into<SharedString>>(
        title: T,
        action: &dyn Action,
        focus: &FocusHandle,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + use<T> {
        Self::builder(
            title.into(),
            Some(action.boxed_clone()),
            Some(focus.clone()),
            None,
        )
    }

    pub fn with_meta<T: Into<SharedString>, M: Into<SharedString>>(
        title: T,
        action: Option<&dyn Action>,
        meta: M,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + use<T, M> {
        Self::builder(
            title.into(),
            action.map(Action::boxed_clone),
            None,
            Some(meta.into()),
        )
    }

    fn builder(
        title: SharedString,
        action: Option<Box<dyn Action>>,
        focus: Option<FocusHandle>,
        meta: Option<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView {
        move |window, cx| {
            let _group = tracing::info_span!("tooltip-show", title = %title).entered();
            let keys = action.as_ref().and_then(|action| {
                if let Some(focus) = focus.as_ref() {
                    window
                        .highest_precedence_binding_for_action_in(action.as_ref(), focus)
                        .or_else(|| window.highest_precedence_binding_for_action(action.as_ref()))
                        .map(|binding| ui::text_for_keybinding_keystrokes(binding.keystrokes(), cx))
                } else {
                    crate::key_hint_text(action.as_ref(), window, cx)
                }
            });
            let key_binding = action.as_ref().map(|action| match focus.as_ref() {
                Some(focus) => ui::KeyBinding::for_action_in(action.as_ref(), focus, cx),
                None => ui::KeyBinding::for_action(action.as_ref(), cx),
            });
            let inner = cx.new(|_| {
                let tooltip = ui::Tooltip::new(title.clone()).key_binding(key_binding);
                match meta.as_ref() {
                    Some(meta) => tooltip.meta(meta.clone()),
                    None => tooltip,
                }
            });
            let view = cx.new(|_| Self { inner });
            tracing::debug!(
                keys = keys.as_deref(),
                meta = meta.is_some(),
                "tooltip built"
            );
            view.into()
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.inner.clone()
    }
}
