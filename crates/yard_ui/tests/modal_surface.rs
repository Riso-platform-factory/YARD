use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    VisualTestContext, Window, div, px, size,
};
use yard_ui::ModalSurface;

struct Surface;

impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        yard_ui::v_flex().size_full().items_start().child(
            div()
                .w(px(200.))
                .flex_none()
                .debug_selector(|| "FRAME".into())
                .child(
                    ModalSurface::new()
                        .w(px(200.))
                        .child(div().debug_selector(|| "FIRST".into()).w_full().h(px(20.)))
                        .child(div().debug_selector(|| "SECOND".into()).w_full().h(px(30.))),
                ),
        )
    }
}

#[gpui::test]
fn children_fill_the_surface_without_padding(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let window = cx.open_window(size(px(400.), px(300.)), |_, _| Surface);
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let frame = cx.debug_bounds("FRAME").expect("surface draws");
    let first = cx.debug_bounds("FIRST").expect("first child draws");
    let second = cx.debug_bounds("SECOND").expect("second child draws");
    // elevation_3 supplies a one-pixel border, with no padding inside it.
    assert_eq!(first.origin, frame.origin + gpui::point(px(1.), px(1.)));
    assert_eq!(first.size, size(px(198.), px(20.)));
    assert_eq!(second.top(), first.bottom());
    assert_eq!(second.size, size(px(198.), px(30.)));
    assert_eq!(frame.size.height, px(52.));
}
