//! Adapts Zed's `platform_title_bar`, `title_bar`, and `ui` crates at revision
//! ddb90e5e89d346f21847281727e1644b8bb2cc37, licensed GPL-3.0-or-later.
//! Copyright Zed Industries, Inc.

use crate::h_flex;
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, TitlebarOptions, Window,
    WindowControlArea, point, prelude::FluentBuilder, px,
};
use theme::ActiveTheme;

/// Window title-bar options for a custom-drawn transparent bar.
pub fn titlebar_options(title: impl Into<SharedString>) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(title.into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(9.), px(9.))),
    }
}

/// A window title bar with `left`, `center`, and `right` item groups.
#[derive(IntoElement)]
pub struct TitleBar {
    id: ElementId,
    left: Vec<AnyElement>,
    center: Vec<AnyElement>,
    right: Vec<AnyElement>,
}

impl TitleBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            left: Vec::new(),
            center: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn left(mut self, child: impl IntoElement) -> Self {
        self.left.push(child.into_any_element());
        self
    }

    pub fn center(mut self, child: impl IntoElement) -> Self {
        self.center.push(child.into_any_element());
        self
    }

    pub fn right(mut self, child: impl IntoElement) -> Self {
        self.right.push(child.into_any_element());
        self
    }
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let height = ui::utils::platform_title_bar_height(window);
        let mac_traffic_lights =
            cfg!(target_os = "macos") && !window.is_fullscreen() && !window.is_simple_fullscreen();

        h_flex()
            .id(self.id)
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .flex_none()
            .h(height)
            .bg(cx.theme().colors().title_bar_background)
            .when(cfg!(target_os = "macos"), |bar| {
                bar.on_click(|event, window, _| {
                    if event.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                })
            })
            .child(
                h_flex().h_full().flex_1().child(
                    h_flex()
                        .h_full()
                        .map(|bar| {
                            if mac_traffic_lights {
                                bar.pl(px(ui::utils::TRAFFIC_LIGHT_PADDING))
                            } else {
                                bar.pl_2()
                            }
                        })
                        .when(!self.left.is_empty(), |bar| {
                            bar.child(
                                h_flex()
                                    .h_full()
                                    .gap_1()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .children(self.left),
                            )
                        }),
                ),
            )
            .when(!self.center.is_empty(), |bar| {
                bar.child(
                    h_flex()
                        .h_full()
                        .gap_1()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children(self.center),
                )
            })
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .justify_end()
                    .child(
                        h_flex()
                            .h_full()
                            .pr_2()
                            .when(!self.right.is_empty(), |bar| {
                                bar.child(
                                    h_flex()
                                        .h_full()
                                        .gap_1()
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .children(self.right),
                                )
                            }),
                    ),
            )
    }
}
