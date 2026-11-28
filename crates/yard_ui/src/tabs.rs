//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    AnyElement, AnyView, App, AppContext, Context, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, Pixels, Render, RenderOnce, Role, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;
use ui::{ButtonCommon, Clickable, LabelCommon, Toggleable, VisibleOnHover};

/// The height a tab bar gives itself, as Zed's `TabBar` does.
pub fn tab_bar_height(cx: &App) -> Pixels {
    ui::Tab::container_height(cx)
}

/// The accessible name of a tab's close button.
fn tab_close_label(label: &str) -> SharedString {
    format!("Close {label}").into()
}

type OnClick = Box<dyn Fn(&mut Window, &mut App)>;
type OnClose = Rc<dyn Fn(&mut Window, &mut App)>;
type TabModifier = Box<dyn FnOnce(ui::Tab) -> ui::Tab>;
type BarModifier = Box<dyn FnOnce(Stateful<Div>) -> Stateful<Div>>;

struct DragPreview(AnyView);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// A tab with a label, selection state, click handler, optional close button, and drag-and-drop hooks.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    selected: bool,
    icon: Option<crate::IconName>,
    on_click: Option<OnClick>,
    on_close: Option<OnClose>,
    modifiers: Vec<TabModifier>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: false,
            icon: None,
            on_click: None,
            on_close: None,
            modifiers: Vec::new(),
        }
    }

    pub fn icon(mut self, icon: crate::IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }

    pub fn on_close(mut self, on_close: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(on_close));
        self
    }

    pub fn on_drag<T: 'static>(
        mut self,
        value: T,
        preview: impl Fn(&T, &mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.modifiers.push(Box::new(move |tab| {
            tab.on_drag(value, move |value, _, window, cx| {
                let view = preview(value, window, cx);
                cx.new(|_| DragPreview(view))
            })
        }));
        self
    }

    pub fn on_drop<T: 'static>(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.modifiers.push(Box::new(move |tab| {
            tab.drag_over::<T>(|style, _, _, cx| style.bg(crate::palette(cx).drop_target))
                .on_drop(handler)
        }));
        self
    }

    fn element(
        self,
        position: ui::TabPosition,
        index: usize,
        accessibility_active: bool,
    ) -> ui::Tab {
        let icon_id = self.id.clone();
        let content = ui::h_flex()
            .gap_1()
            .children(self.icon.map(|icon| {
                div()
                    .debug_selector(move || format!("ICON-{icon_id}"))
                    .child(
                        ui::Icon::new(icon.zed())
                            .size(ui::IconSize::Small)
                            .color(ui::Color::Muted),
                    )
            }))
            .child(
                ui::Label::new(self.label.clone())
                    .single_line()
                    .color(if self.selected {
                        ui::Color::Default
                    } else {
                        ui::Color::Muted
                    }),
            );
        let close_id = self.id.clone();
        let tab = ui::Tab::new(self.id)
            .role(Role::Tab)
            .aria_label(self.label.clone())
            .aria_selected(self.selected)
            .aria_position_in_set(index)
            .position(position)
            .toggle_state(self.selected)
            .child(content);
        let mut tab = match self.on_click {
            Some(on_click) => tab.on_click(move |_, window, cx| on_click(window, cx)),
            None => tab,
        };
        if let Some(on_close) = self.on_close {
            let close = on_close.clone();
            let button = ui::IconButton::new("close tab", ui::IconName::Close)
                .aria_label(tab_close_label(&self.label))
                .shape(ui::IconButtonShape::Square)
                .icon_color(ui::Color::Muted)
                .size(ui::ButtonSize::None)
                .icon_size(ui::IconSize::Small)
                .tooltip(crate::Tooltip::text("Close Tab"))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(window, cx);
                });
            // GPUI dispatches accessibility clicks through the button hitbox.
            let button = if accessibility_active {
                button
            } else {
                button.visible_on_hover("")
            };
            tab = tab
                .end_slot(
                    div()
                        .debug_selector(move || format!("CLOSE-{close_id}"))
                        .child(button),
                )
                .on_aux_click(move |event, window, cx| {
                    if event.is_middle_click() {
                        cx.stop_propagation();
                        on_close(window, cx);
                    }
                });
        }
        for modify in self.modifiers {
            tab = modify(tab);
        }
        tab
    }
}

impl RenderOnce for Tab {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        self.element(ui::TabPosition::First, 0, window.is_a11y_active())
    }
}

/// A bar that arranges its tabs around the selected one.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<Tab>,
    drop_handlers: Vec<BarModifier>,
    scroll_handle: Option<ScrollHandle>,
    end_children: Vec<AnyElement>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            drop_handlers: Vec::new(),
            scroll_handle: None,
            end_children: Vec::new(),
        }
    }

    /// Lays the tabs in a row that scrolls along `handle`, as Zed's `TabBar` does.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_handle = Some(handle.clone());
        self
    }

    /// Adds an element at the bar's end, after the scrolling row, spaced as
    /// Zed's `TabBar` end slot.
    pub fn end_child(mut self, child: impl IntoElement) -> Self {
        self.end_children.push(child.into_any_element());
        self
    }

    pub fn tabs(mut self, tabs: Vec<Tab>) -> Self {
        self.tabs = tabs;
        self
    }

    pub fn on_drop<T: 'static>(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.drop_handlers.push(Box::new(move |bar| {
            bar.drag_over::<T>(|style, _, _, cx| style.bg(crate::palette(cx).drop_target))
                .on_drop(handler)
        }));
        self
    }

    fn element(self, accessibility_active: bool, cx: &App) -> Stateful<Div> {
        let selected_index = self.tabs.iter().position(|tab| tab.selected).unwrap_or(0);
        let count = self.tabs.len();
        let bar_id = self.id.clone();
        let bar = ui::TabBar::new("tabs").children(self.tabs.into_iter().enumerate().map(
            |(index, tab)| {
                let position = if index == 0 {
                    ui::TabPosition::First
                } else if index == count - 1 {
                    ui::TabPosition::Last
                } else {
                    ui::TabPosition::Middle(index.cmp(&selected_index))
                };
                tab.element(position, index, accessibility_active)
            },
        ));
        let bar = match &self.scroll_handle {
            Some(handle) => bar.track_scroll(handle),
            None => bar,
        };
        let bar = bar.end_children(self.end_children);
        let bar = if self.drop_handlers.is_empty() {
            bar
        } else {
            let mut target = div()
                .id("tab_bar_drop_target")
                .debug_selector(move || format!("TABBAR-{bar_id}"))
                .min_w_6()
                .h(ui::Tab::container_height(cx))
                .flex_grow(1.)
                .child("");
            for modify in self.drop_handlers {
                target = modify(target);
            }
            bar.child(target)
        };
        div()
            .id(self.id)
            .role(Role::TabList)
            // The AT-SPI and UI Automation adapters look up the count on ancestors.
            .aria_size_of_set(count)
            .flex()
            .flex_none()
            .w_full()
            .child(bar)
    }
}

impl RenderOnce for TabBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.element(window.is_a11y_active(), cx)
    }
}

#[cfg(test)]
mod tests;
