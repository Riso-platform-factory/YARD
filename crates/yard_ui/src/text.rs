//! Wraps Zed's `ui` and `theme` crates at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{App, Hsla, IntoElement, Rems, RenderOnce, SharedString, Window, rems};
use theme::ActiveTheme;
use ui::LabelCommon;

/// The size of a [`Label`], in rems.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelSize {
    Large,
    #[default]
    Default,
    Small,
    XSmall,
}

impl LabelSize {
    pub fn rems(self) -> Rems {
        match self {
            Self::Large => rems(1.),
            Self::Default => rems(0.875),
            Self::Small => rems(0.75),
            Self::XSmall => rems(0.625),
        }
    }

    fn zed(self) -> ui::LabelSize {
        match self {
            Self::Large => ui::LabelSize::Large,
            Self::Default => ui::LabelSize::Default,
            Self::Small => ui::LabelSize::Small,
            Self::XSmall => ui::LabelSize::XSmall,
        }
    }
}

/// The size of a [`Headline`], in rems.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeadlineSize {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

impl HeadlineSize {
    pub fn rems(self) -> Rems {
        match self {
            Self::XSmall => rems(0.88),
            Self::Small => rems(1.),
            Self::Medium => rems(1.125),
            Self::Large => rems(1.27),
            Self::XLarge => rems(1.43),
        }
    }

    fn zed(self) -> ui::HeadlineSize {
        match self {
            Self::XSmall => ui::HeadlineSize::XSmall,
            Self::Small => ui::HeadlineSize::Small,
            Self::Medium => ui::HeadlineSize::Medium,
            Self::Large => ui::HeadlineSize::Large,
            Self::XLarge => ui::HeadlineSize::XLarge,
        }
    }
}

/// Semantic text and status colors from the active theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextColor {
    #[default]
    Default,
    Muted,
    Accent,
    Disabled,
    Placeholder,
    Error,
    Warning,
    Success,
}

impl TextColor {
    pub fn hsla(self, cx: &App) -> Hsla {
        match self {
            Self::Default => cx.theme().colors().text,
            Self::Muted => cx.theme().colors().text_muted,
            Self::Accent => cx.theme().colors().text_accent,
            Self::Disabled => cx.theme().colors().text_disabled,
            Self::Placeholder => cx.theme().colors().text_placeholder,
            Self::Error => cx.theme().status().error,
            Self::Warning => cx.theme().status().warning,
            Self::Success => cx.theme().status().success,
        }
    }

    fn zed(self) -> ui::Color {
        match self {
            Self::Default => ui::Color::Default,
            Self::Muted => ui::Color::Muted,
            Self::Accent => ui::Color::Accent,
            Self::Disabled => ui::Color::Disabled,
            Self::Placeholder => ui::Color::Placeholder,
            Self::Error => ui::Color::Error,
            Self::Warning => ui::Color::Warning,
            Self::Success => ui::Color::Success,
        }
    }
}

/// A text label in Zed's sizes and colors.
#[derive(IntoElement)]
pub struct Label {
    inner: ui::Label,
    size: LabelSize,
    color: TextColor,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            inner: ui::Label::new(text),
            size: LabelSize::default(),
            color: TextColor::default(),
        }
    }

    pub fn size(mut self, size: LabelSize) -> Self {
        self.size = size;
        self.inner = self.inner.size(size.zed());
        self
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = color;
        self.inner = self.inner.color(color.zed());
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        tracing::trace!(size = ?self.size, color = ?self.color, "rendering label");
        self.inner
    }
}

/// A heading in Zed's headline sizes and the theme's text color.
#[derive(IntoElement)]
pub struct Headline {
    inner: ui::Headline,
    size: HeadlineSize,
}

impl Headline {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            inner: ui::Headline::new(text),
            size: HeadlineSize::default(),
        }
    }

    pub fn size(mut self, size: HeadlineSize) -> Self {
        self.size = size;
        self.inner = self.inner.size(size.zed());
        self
    }
}

impl RenderOnce for Headline {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        tracing::trace!(size = ?self.size, "rendering headline");
        self.inner
    }
}

#[cfg(test)]
mod tests;
