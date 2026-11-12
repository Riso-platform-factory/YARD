//! Wraps Zed's `ui` crate at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use gpui::{
    AnyView, App, AppContext, Context, Div, ElementId, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Window, div,
};
use ui::Toggleable;

type OnClick = Box<dyn Fn(&mut Window, &mut App)>;
type TabModifier = Box<dyn FnOnce(ui::Tab) -> ui::Tab>;
type BarModifier = Box<dyn FnOnce(Stateful<Div>) -> Stateful<Div>>;

struct DragPreview(AnyView);

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

/// A tab with a label, selection state, click handler, and drag-and-drop hooks.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<OnClick>,
    modifiers: Vec<TabModifier>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: false,
            on_click: None,
            modifiers: Vec::new(),
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

    fn element(self, position: ui::TabPosition) -> ui::Tab {
        let tab = ui::Tab::new(self.id)
            .position(position)
            .toggle_state(self.selected)
            .child(ui::Label::new(self.label));
        let mut tab = match self.on_click {
            Some(on_click) => tab.on_click(move |_, window, cx| on_click(window, cx)),
            None => tab,
        };
        for modify in self.modifiers {
            tab = modify(tab);
        }
        tab
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
    drop_handlers: Vec<BarModifier>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            drop_handlers: Vec::new(),
        }
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
}

impl RenderOnce for TabBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let selected_index = self.tabs.iter().position(|tab| tab.selected).unwrap_or(0);
        let count = self.tabs.len();
        let bar_id = self.id.clone();
        let bar = ui::TabBar::new(self.id).children(self.tabs.into_iter().enumerate().map(
            |(index, tab)| {
                let position = if index == 0 {
                    ui::TabPosition::First
                } else if index == count - 1 {
                    ui::TabPosition::Last
                } else {
                    ui::TabPosition::Middle(index.cmp(&selected_index))
                };
                tab.element(position)
            },
        ));
        if self.drop_handlers.is_empty() {
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
        }
    }
}
