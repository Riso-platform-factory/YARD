//! Adapts Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use crate::{IconButton, IconName, IconSize, h_flex, v_flex};
use gpui::{
    AnyElement, App, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Rems, RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Window,
    prelude::FluentBuilder, rems,
};
use ui::{DynamicSpacing, LabelCommon, StyledExt};

type Dismiss = Box<dyn Fn(&mut Window, &mut App)>;

/// A modal surface drawn where its owner places it.
#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    width: Rems,
    aria_label: Option<SharedString>,
    focus: Option<FocusHandle>,
    header: Option<ModalHeader>,
    sections: Vec<Section>,
    footer: Option<ModalFooter>,
}

impl Modal {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: rems(30.),
            aria_label: None,
            focus: None,
            header: None,
            sections: Vec::new(),
            footer: None,
        }
    }

    pub fn width(mut self, width: Rems) -> Self {
        self.width = width;
        self
    }

    /// Names a dialog whose header has no headline.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// Tracks keyboard focus on the dialog itself.
    pub fn track_focus(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }

    pub fn header(mut self, header: ModalHeader) -> Self {
        self.header = Some(header);
        self
    }

    pub fn section(mut self, section: Section) -> Self {
        self.sections.push(section);
        self
    }

    pub fn footer(mut self, footer: ModalFooter) -> Self {
        self.footer = Some(footer);
        self
    }
}

impl RenderOnce for Modal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let header = self.header.unwrap_or_default();
        tracing::trace!(
            id = %self.id,
            headline = header.headline.as_deref().unwrap_or_default(),
            description = header.description.is_some(),
            close_button = header.on_dismiss.is_some(),
            sections = self.sections.len(),
            footer = self.footer.is_some(),
            "rendering modal",
        );
        let has_header = header.headline.is_some()
            || header.description.is_some()
            || header.on_dismiss.is_some();
        let name = header.headline.clone().or(self.aria_label);
        v_flex()
            .id(self.id.clone())
            .role(Role::Dialog)
            .when_some(name, |dialog, name| dialog.aria_label(name))
            .when_some(self.focus, |dialog, focus| dialog.track_focus(&focus))
            .elevation_3(cx)
            .w(self.width)
            .overflow_hidden()
            .when(has_header, |this| this.child(header.render(self.id, cx)))
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .gap(DynamicSpacing::Base08.rems(cx))
                    .children(
                        self.sections
                            .into_iter()
                            .enumerate()
                            .map(|(index, section)| {
                                v_flex()
                                    .id(("section", index))
                                    .w_full()
                                    .flex_1()
                                    .child(section)
                            }),
                    ),
            )
            .children(self.footer)
    }
}

/// A headline, description, and optional owner-handled close button.
#[derive(Default)]
pub struct ModalHeader {
    headline: Option<SharedString>,
    description: Option<SharedString>,
    on_dismiss: Option<Dismiss>,
}

impl ModalHeader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn headline(mut self, headline: impl Into<SharedString>) -> Self {
        self.headline = Some(headline.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn on_dismiss(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Box::new(callback));
        self
    }

    fn render(self, modal_id: ElementId, cx: &App) -> Div {
        h_flex()
            .min_w_0()
            .flex_none()
            .justify_between()
            .w_full()
            .px(DynamicSpacing::Base12.rems(cx))
            .pt(DynamicSpacing::Base08.rems(cx))
            .pb(DynamicSpacing::Base04.rems(cx))
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .justify_between()
                            .child(h_flex().gap_1().when_some(self.headline, |this, headline| {
                                this.child(
                                    ui::Headline::new(headline)
                                        .size(ui::HeadlineSize::Small)
                                        .color(ui::Color::Muted),
                                )
                            }))
                            .when_some(self.on_dismiss, |this, callback| {
                                this.child(
                                    IconButton::new(
                                        SharedString::from(format!("{modal_id}-dismiss")),
                                        IconName::Close,
                                    )
                                    .aria_label("Close")
                                    .icon_size(IconSize::Small)
                                    .on_click(callback),
                                )
                            }),
                    )
                    .when_some(self.description, |this, description| {
                        this.child(
                            ui::Label::new(description)
                                .color(ui::Color::Muted)
                                .mb_2()
                                .flex_1(),
                        )
                    }),
            )
    }
}

/// An uncontained section with optional header and arbitrary children.
#[derive(IntoElement)]
pub struct Section {
    inner: ui::Section,
}

impl Default for Section {
    fn default() -> Self {
        Self::new()
    }
}

impl Section {
    pub fn new() -> Self {
        Self {
            inner: ui::Section::new(),
        }
    }

    pub fn header(mut self, header: SectionHeader) -> Self {
        self.inner = self.inner.header(header.inner);
        self
    }
}

impl ParentElement for Section {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl RenderOnce for Section {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.inner
    }
}

/// A small section label.
pub struct SectionHeader {
    inner: ui::SectionHeader,
}

impl SectionHeader {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            inner: ui::SectionHeader::new(label),
        }
    }
}

/// A bordered footer with left and right slots.
#[derive(IntoElement)]
pub struct ModalFooter {
    inner: ui::ModalFooter,
}

impl Default for ModalFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalFooter {
    pub fn new() -> Self {
        Self {
            inner: ui::ModalFooter::new(),
        }
    }

    pub fn start_slot(mut self, slot: impl IntoElement) -> Self {
        self.inner = self.inner.start_slot(slot);
        self
    }

    pub fn end_slot(mut self, slot: impl IntoElement) -> Self {
        self.inner = self.inner.end_slot(slot);
        self
    }
}

impl RenderOnce for ModalFooter {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.inner
    }
}

#[cfg(test)]
mod tests;
