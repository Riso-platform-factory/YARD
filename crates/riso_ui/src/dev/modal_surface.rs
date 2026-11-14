//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    AnyElement, App, Div, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
};
use ui::StyledExt;

/// An elevated modal frame with no padding around its children.
#[derive(IntoElement)]
pub struct ModalSurface {
    base: Div,
}

impl Default for ModalSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalSurface {
    pub fn new() -> Self {
        Self {
            base: crate::v_flex(),
        }
    }
}

impl ParentElement for ModalSurface {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl Styled for ModalSurface {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for ModalSurface {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        tracing::trace!("rendering modal surface");
        self.base.elevation_3(cx)
    }
}
