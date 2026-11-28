//! Wraps Zed's `ui::Switch` at revision ddb90e5, licensed GPL-3.0-or-later.

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Window, div,
};

/// A named on/off switch. The caller supplies its current state.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    inner: ui::Switch,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Self {
        let id = id.into();
        Self {
            inner: ui::Switch::new(id.clone(), on.into()).aria_label(label),
            id,
        }
    }

    pub fn on_toggle(mut self, callback: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.inner = self
            .inner
            .tab_index(0_isize)
            .on_click(move |state, window, cx| {
                // Zed registers the same callback on the focusable switch and its outer row.
                cx.stop_propagation();
                callback(*state == ui::ToggleState::Selected, window, cx);
            });
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        tracing::trace!(id = %self.id, "rendering switch");
        div()
            .debug_selector(|| self.id.to_string())
            .child(self.inner)
    }
}
