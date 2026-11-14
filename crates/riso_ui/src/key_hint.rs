//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{Action, App, FocusHandle, IntoElement, RenderOnce, Window};

/// How the keys bound to an action are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyHintStyle {
    #[default]
    Default,
    Label,
}

/// The keys bound to an action, drawn as Zed draws them.
#[derive(IntoElement)]
pub struct KeyHint {
    inner: ui::KeyBinding,
}

impl KeyHint {
    pub fn for_action(action: &dyn Action, cx: &App) -> Self {
        Self {
            inner: ui::KeyBinding::for_action(action, cx),
        }
    }

    pub fn for_action_in(action: &dyn Action, focus: &FocusHandle, cx: &App) -> Self {
        Self {
            inner: ui::KeyBinding::for_action_in(action, focus, cx),
        }
    }

    pub fn style(mut self, style: KeyHintStyle) -> Self {
        self.inner = self.inner.style(match style {
            KeyHintStyle::Default => ui::KeyBindingStyle::Default,
            KeyHintStyle::Label => ui::KeyBindingStyle::Label,
        });
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.inner = self.inner.disabled(disabled);
        self
    }
}

impl RenderOnce for KeyHint {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.inner
    }
}

/// The keys bound to an action as platform-specific words.
pub fn key_hint_text(action: &dyn Action, window: &Window, cx: &App) -> Option<String> {
    ui::text_for_action(action, window, cx)
}
