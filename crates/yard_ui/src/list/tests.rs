use super::*;
use crate::{Label, v_flex};
use gpui::{InteractiveElement, Styled, TestAppContext, div, px, size};
use std::{cell::Cell, rc::Rc};

#[test]
fn default_spacing_is_dense() {
    assert_eq!(ListItemSpacing::default(), ListItemSpacing::Dense);
}

struct BuilderVariants {
    clicks: Rc<Cell<usize>>,
}

impl gpui::Render for BuilderVariants {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let clicks = self.clicks.clone();
        v_flex()
            .size_full()
            .child(
                ListHeader::new("Standalone")
                    .inset(true)
                    .end_slot(Label::new("3")),
            )
            .child(
                List::default()
                    .header(
                        ListHeader::new("Inset header")
                            .inset(true)
                            .start_slot(Label::new("*"))
                            .end_slot(Label::new("end")),
                    )
                    .child(
                        div().debug_selector(|| "DISABLED_AFTER".into()).child(
                            ListItem::new("disabled-after")
                                .inset(true)
                                .selectable(false)
                                .on_click(move |_, _| clicks.set(clicks.get() + 1))
                                .disabled(true)
                                .child(Label::new("Disabled after on_click")),
                        ),
                    )
                    .child(ListSeparator),
            )
    }
}

#[gpui::test]
fn disabling_after_on_click_still_suppresses_handler(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let window = cx.open_window(size(px(400.), px(600.)), |_, _| BuilderVariants {
        clicks: counted,
    });
    let mut cx = gpui::VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = cx
        .debug_bounds("DISABLED_AFTER")
        .expect("disabled item draws");
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    assert_eq!(clicks.get(), 0);
}
