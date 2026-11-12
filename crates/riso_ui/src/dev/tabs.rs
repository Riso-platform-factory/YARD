//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Window,
};
use ui::Toggleable;

type OnClick = Box<dyn Fn(&mut Window, &mut App)>;

/// A tab with a label, selection state, and click handler.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<OnClick>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: false,
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }

    fn element(self, position: ui::TabPosition) -> ui::Tab {
        let tab = ui::Tab::new(self.id)
            .position(position)
            .toggle_state(self.selected)
            .child(ui::Label::new(self.label));
        match self.on_click {
            Some(on_click) => tab.on_click(move |_, window, cx| on_click(window, cx)),
            None => tab,
        }
    }
}

impl RenderOnce for Tab {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.element(ui::TabPosition::First)
    }
}

/// A bar that arranges its tabs around the selected one.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<Tab>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
        }
    }

    pub fn tabs(mut self, tabs: Vec<Tab>) -> Self {
        self.tabs = tabs;
        self
    }
}

impl RenderOnce for TabBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let selected_index = self.tabs.iter().position(|tab| tab.selected).unwrap_or(0);
        let count = self.tabs.len();
        ui::TabBar::new(self.id).children(self.tabs.into_iter().enumerate().map(|(index, tab)| {
            let position = if index == 0 {
                ui::TabPosition::First
            } else if index == count - 1 {
                ui::TabPosition::Last
            } else {
                ui::TabPosition::Middle(index.cmp(&selected_index))
            };
            tab.element(position)
        }))
    }
}
