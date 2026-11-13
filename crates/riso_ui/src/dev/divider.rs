//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{App, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use theme::ActiveTheme;

#[derive(Clone, Copy, Debug)]
enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug)]
enum Style {
    Solid,
    Dashed,
}

/// The color of a [`Divider`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DividerColor {
    Border,
    BorderFaded,
    #[default]
    BorderVariant,
}

impl DividerColor {
    pub fn hsla(self, cx: &App) -> Hsla {
        match self {
            Self::Border => cx.theme().colors().border,
            Self::BorderFaded => cx.theme().colors().border.opacity(0.6),
            Self::BorderVariant => cx.theme().colors().border_variant,
        }
    }

    fn zed(self) -> ui::DividerColor {
        match self {
            Self::Border => ui::DividerColor::Border,
            Self::BorderFaded => ui::DividerColor::BorderFaded,
            Self::BorderVariant => ui::DividerColor::BorderVariant,
        }
    }
}

/// A thin horizontal or vertical line between groups.
#[derive(IntoElement)]
pub struct Divider {
    inner: ui::Divider,
    direction: Direction,
    style: Style,
    color: DividerColor,
    inset: bool,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self {
            inner: ui::Divider::horizontal(),
            direction: Direction::Horizontal,
            style: Style::Solid,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn vertical() -> Self {
        Self {
            inner: ui::Divider::vertical().h_full().self_stretch(),
            direction: Direction::Vertical,
            style: Style::Solid,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn horizontal_dashed() -> Self {
        Self {
            inner: ui::Divider::horizontal_dashed(),
            direction: Direction::Horizontal,
            style: Style::Dashed,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn vertical_dashed() -> Self {
        Self {
            inner: ui::Divider::vertical_dashed().h_full().self_stretch(),
            direction: Direction::Vertical,
            style: Style::Dashed,
            color: DividerColor::default(),
            inset: false,
        }
    }

    pub fn inset(mut self) -> Self {
        self.inset = true;
        self.inner = match self.direction {
            Direction::Horizontal => self.inner.inset().w_auto().self_stretch(),
            Direction::Vertical => self.inner.inset().h_auto().self_stretch(),
        };
        self
    }

    pub fn color(mut self, color: DividerColor) -> Self {
        self.color = color;
        self.inner = self.inner.color(color.zed());
        self
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        tracing::trace!(
            direction = ?self.direction,
            style = ?self.style,
            color = ?self.color,
            "rendering divider",
        );
        match (self.direction, self.inset) {
            (Direction::Vertical, true) => div()
                .flex()
                .min_w_0()
                .w_px()
                .min_h_full()
                .self_stretch()
                .child(self.inner)
                .into_any_element(),
            _ => self.inner.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests;
