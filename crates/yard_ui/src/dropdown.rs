//! Uses Zed's popover, context menu, and button at revision ddb90e5 (GPL-3.0-or-later).

use gpui::{
    Anchor, App, ClickEvent, CursorStyle, ElementId, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, Role, SharedString, Window, div, px,
};
use std::rc::Rc;
use ui::{ButtonCommon, Clickable, Disableable, Toggleable};

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// A named dropdown that reports the index of the chosen option.
#[derive(IntoElement)]
pub struct Dropdown {
    id: ElementId,
    label: SharedString,
    options: Vec<SharedString>,
    selected: Option<usize>,
    on_select: Option<OnSelect>,
}

impl Dropdown {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        options: Vec<SharedString>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options,
            selected,
            on_select: None,
        }
    }

    pub fn on_select(mut self, callback: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(callback));
        self
    }
}

// PopoverMenu communicates whether it is open via Toggleable. Use expansion
// here because a toggled state takes precedence over the value on macOS.
#[derive(IntoElement)]
struct Trigger(ui::Button);

impl Toggleable for Trigger {
    fn toggle_state(self, expanded: bool) -> Self {
        Self(self.0.aria_expanded(expanded))
    }
}

impl Clickable for Trigger {
    fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self(self.0.on_click(handler))
    }

    fn cursor_style(self, style: CursorStyle) -> Self {
        Self(self.0.cursor_style(style))
    }
}

impl RenderOnce for Trigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}

impl RenderOnce for Dropdown {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        tracing::trace!(id = %self.id, options = self.options.len(), "rendering dropdown");
        let id = self.id;
        let selected = self
            .selected
            .and_then(|index| self.options.get(index))
            .cloned();
        let enabled = !self.options.is_empty() && self.on_select.is_some();
        let handle = ui::PopoverMenuHandle::<ui::ContextMenu>::default();
        let trigger = ui::Button::new(
            id.clone(),
            selected.clone().unwrap_or_else(|| "Select…".into()),
        )
        .aria_role(Role::ComboBox)
        .aria_label(self.label)
        .aria_value(selected.unwrap_or_default())
        .end_icon(ui::Icon::new(crate::IconName::ChevronDown.zed()).size(ui::IconSize::XSmall))
        .tab_index(0_isize)
        .disabled(!enabled)
        .on_a11y_action(gpui::accesskit::Action::Expand, {
            let handle = handle.clone();
            move |_, window, cx| {
                if enabled && !handle.is_deployed() {
                    handle.show(window, cx);
                }
            }
        })
        .on_a11y_action(gpui::accesskit::Action::Collapse, {
            let handle = handle.clone();
            move |_, _, cx| handle.hide(cx)
        });
        // TODO(settings-ui): Restore focus when the menu is dismissed in the frame it opens. Update Zed once its popover cancels the pending focus transfer.
        // TODO(settings-ui): Keep an open menu aligned with its trigger and sized to the window. Update Zed once its popover follows layout moves and window resizes.
        let popover = ui::PopoverMenu::new((id.clone(), "popover"))
            .with_handle(handle)
            .attach(Anchor::BottomLeft)
            .menu(move |window, cx| {
                let on_select = self.on_select.as_ref().filter(|_| enabled)?;
                // Preserve Zed's popup margins even when an option has a long label.
                let width = (window.viewport_size().width - px(16.))
                    .max(px(1.))
                    .min(window.rem_size() * 20.);
                // Each opening gets its own menu and dismissal subscription.
                Some(ui::ContextMenu::build(window, cx, |menu, window, cx| {
                    let mut menu = menu.fixed_width(width.into());
                    for (index, option) in self.options.iter().enumerate() {
                        let on_select = on_select.clone();
                        menu = menu.toggleable_entry(
                            option.clone(),
                            self.selected == Some(index),
                            ui::IconPosition::Start,
                            None,
                            move |window, cx| on_select(index, window, cx),
                        );
                    }
                    // TODO(settings-ui): Scroll the keyboard selection into view in long menus. Update Zed once its context menu scrolls the selected row into view.
                    menu.select_toggled_or_first(window, cx);
                    menu
                }))
            })
            .trigger(Trigger(trigger));
        div().debug_selector(|| id.to_string()).child(popover)
    }
}
