use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};

// The probe records every `on_resized` call with the shares it was given.
struct Probe {
    axis: gpui::Axis,
    shares: Rc<RefCell<Vec<f32>>>,
    resized: Rc<RefCell<Vec<Vec<f32>>>>,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let children = (0..self.shares.borrow().len())
            .map(|_| gpui::div().size_full().into_any_element())
            .collect();
        let resized = self.resized.clone();
        gpui::div().size_full().child(
            riso_ui::axis("split", self.axis, self.shares.clone(), children)
                .on_resized(move |shares, _, _| resized.borrow_mut().push(shares)),
        )
    }
}

fn draw(window: gpui::WindowHandle<Probe>, cx: &mut gpui::TestAppContext) {
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
}

fn position(axis: gpui::Axis, along: f32) -> gpui::Point<gpui::Pixels> {
    gpui::point(gpui::px(20.), gpui::px(20.)).apply_along(axis, |_| gpui::px(along))
}

// `on_resized` is called once at the mouse up of a drag that changed the
// shares, with the new shares; it is not called by a click without a move; a
// double click calls it once with equal shares.
#[gpui::test]
fn on_resized_is_called_once_on_a_changed_drag_and_a_double_click(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let shares = Rc::new(RefCell::new(vec![1.; 3]));
        let resized = Rc::new(RefCell::new(Vec::new()));
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| Probe {
                    axis,
                    shares: shares.clone(),
                    resized: resized.clone(),
                })
            })
            .unwrap()
        });
        let initial =
            gpui::size(gpui::px(180.), gpui::px(180.)).apply_along(axis, |_| gpui::px(600.));
        cx.simulate_window_resize(window.into(), initial);
        draw(window, cx);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // The first handle sits at the end of the first child's 200 pixels.
        let handle = position(axis, 200.);
        // A click on the handle without a move calls nothing.
        visual.simulate_mouse_move(handle, None, Default::default());
        visual.simulate_mouse_down(handle, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(handle, gpui::MouseButton::Left, Default::default());
        assert!(resized.borrow().is_empty());
        // Two steps of one drag still end in a single call, at the mouse up.
        visual.simulate_mouse_move(handle, None, Default::default());
        visual.simulate_mouse_down(handle, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            position(axis, 225.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        assert!(resized.borrow().is_empty(), "not during the drag");
        visual.simulate_mouse_move(
            position(axis, 250.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_up(
            position(axis, 250.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        assert_eq!(*resized.borrow(), [vec![1.25, 0.75, 1.]]);
        // The handle moved with the first child: it sits at its new edge.
        let handle = position(axis, 250.);
        visual.simulate_mouse_move(handle, None, Default::default());
        visual.simulate_event(gpui::MouseDownEvent {
            position: handle,
            button: gpui::MouseButton::Left,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(handle, gpui::MouseButton::Left, Default::default());
        assert_eq!(*resized.borrow(), [vec![1.25, 0.75, 1.], vec![1., 1., 1.]]);
    }
}
