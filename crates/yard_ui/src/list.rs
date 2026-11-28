//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use ui::{Disableable, Toggleable};

type Click = Box<dyn Fn(&mut Window, &mut App)>;

/// Vertical spacing of a [`ListItem`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListItemSpacing {
    #[default]
    Dense,
    ExtraDense,
    Sparse,
}

impl ListItemSpacing {
    fn zed(self) -> ui::ListItemSpacing {
        match self {
            Self::Dense => ui::ListItemSpacing::Dense,
            Self::ExtraDense => ui::ListItemSpacing::ExtraDense,
            Self::Sparse => ui::ListItemSpacing::Sparse,
        }
    }
}

/// A vertical list of items with an optional header and empty message.
#[derive(IntoElement)]
pub struct List {
    id: Option<ElementId>,
    inner: ui::List,
}

impl Default for List {
    fn default() -> Self {
        Self::new()
    }
}

impl List {
    pub fn new() -> Self {
        Self {
            id: None,
            inner: ui::List::new()
                .empty_message(ui::EmptyMessage::Text(SharedString::new_static("No items"))),
        }
    }

    /// Gives this list a stable ID and exposes it to assistive technology.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn header(mut self, header: ListHeader) -> Self {
        self.inner = self.inner.header(header.inner);
        self
    }

    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.inner = self
            .inner
            .empty_message(ui::EmptyMessage::Text(message.into()));
        self
    }
}

impl ParentElement for List {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl RenderOnce for List {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        match self.id {
            Some(id) => crate::v_flex()
                .id(id)
                .role(Role::List)
                .w_full()
                .child(self.inner)
                .into_any_element(),
            None => self.inner.into_any_element(),
        }
    }
}

/// A single row in a [`List`].
#[derive(IntoElement)]
pub struct ListItem {
    inner: ui::ListItem,
    id: ElementId,
    disabled: bool,
    on_click: Option<Click>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            inner: ui::ListItem::new(id.clone()),
            id,
            disabled: false,
            on_click: None,
        }
    }

    pub fn aria_role(mut self, role: Role) -> Self {
        self.inner = self.inner.aria_role(role);
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.inner = self.inner.aria_label(label);
        self
    }

    /// Sets selection. Zed exposes the same state visually and to assistive technology.
    pub fn aria_selected(self, selected: bool) -> Self {
        self.toggle_state(selected)
    }

    pub fn toggle_state(mut self, selected: bool) -> Self {
        self.inner = self.inner.toggle_state(selected);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.inner = self.inner.disabled(disabled);
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.inner = self.inner.selectable(selectable);
        self
    }

    pub fn spacing(mut self, spacing: ListItemSpacing) -> Self {
        self.inner = self.inner.spacing(spacing.zed());
        self
    }

    pub fn inset(mut self, inset: bool) -> Self {
        self.inner = self.inner.inset(inset);
        self
    }

    pub fn indent_level(mut self, level: usize) -> Self {
        self.inner = self.inner.indent_level(level);
        self
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.inner = self.inner.start_slot(element);
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.inner = self.inner.end_slot(element);
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl RenderOnce for ListItem {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut inner = self.inner;
        if !self.disabled
            && let Some(handler) = self.on_click
        {
            let id = self.id;
            inner = inner.on_click(move |_, window, cx| {
                let _group = tracing::debug_span!("list-item-click", id = %id).entered();
                handler(window, cx);
            });
        }
        inner
    }
}

/// A header row for a [`List`].
#[derive(IntoElement)]
pub struct ListHeader {
    inner: ui::ListHeader,
}

impl ListHeader {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            inner: ui::ListHeader::new(label),
        }
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.inner = self.inner.start_slot(element);
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.inner = self.inner.end_slot(element);
        self
    }

    pub fn inset(mut self, inset: bool) -> Self {
        self.inner = self.inner.inset(inset);
        self
    }
}

impl RenderOnce for ListHeader {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.inner
    }
}

/// A thin horizontal line between groups of items in a [`List`].
#[derive(IntoElement)]
pub struct ListSeparator;

impl RenderOnce for ListSeparator {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        ui::ListSeparator
    }
}

#[cfg(test)]
mod tests;
