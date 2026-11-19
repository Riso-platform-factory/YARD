use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};
use yard_ui::{AxisChild, AxisSize};

struct Probe {
    axis: gpui::Axis,
    children: Rc<RefCell<Vec<AxisChild>>>,
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let children = (0..self.children.borrow().len())
            .map(|_| {
                let bounds = self.bounds.clone();
                gpui::canvas(
                    move |value, _, _| bounds.borrow_mut().push(value),
                    |_, _, _, _| {},
                )
                .size_full()
                .into_any_element()
            })
            .collect();
        gpui::div().size_full().child(yard_ui::axis(
            "split",
            self.axis,
            self.children.clone(),
            children,
        ))
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

fn open(
    cx: &mut gpui::TestAppContext,
    axis: gpui::Axis,
    children: Rc<RefCell<Vec<AxisChild>>>,
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
    length: f32,
) -> gpui::WindowHandle<Probe> {
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), |_, cx| {
            cx.new(|_| Probe {
                axis,
                children,
                bounds,
            })
        })
        .unwrap()
    });
    resize(window, cx, axis, length);
    window
}

fn resize(
    window: gpui::WindowHandle<Probe>,
    cx: &mut gpui::TestAppContext,
    axis: gpui::Axis,
    length: f32,
) {
    let size = gpui::size(gpui::px(200.), gpui::px(200.)).apply_along(axis, |_| gpui::px(length));
    cx.simulate_window_resize(window.into(), size);
    draw(window, cx);
}

fn assert_sizes(
    bounds: &RefCell<Vec<gpui::Bounds<gpui::Pixels>>>,
    axis: gpui::Axis,
    expected: &[f32],
) {
    let bounds = bounds.borrow();
    assert_eq!(bounds.len(), expected.len());
    for (bounds, expected) in bounds.iter().zip(expected) {
        assert_eq!(bounds.size.along(axis), gpui::px(*expected));
    }
}

fn position(axis: gpui::Axis, along: f32) -> gpui::Point<gpui::Pixels> {
    gpui::point(gpui::px(20.), gpui::px(20.)).apply_along(axis, |_| gpui::px(along))
}

fn with_sizes(sizes: &[(AxisSize, f32)]) -> Rc<RefCell<Vec<AxisChild>>> {
    Rc::new(RefCell::new(
        sizes
            .iter()
            .map(|(size, min)| AxisChild {
                size: *size,
                min: gpui::px(*min),
                collapsed: false,
            })
            .collect(),
    ))
}

fn drag(
    window: gpui::WindowHandle<Probe>,
    cx: &mut gpui::TestAppContext,
    axis: gpui::Axis,
    from: f32,
    to: f32,
) {
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    visual.simulate_mouse_move(position(axis, from), None, Default::default());
    visual.simulate_mouse_down(
        position(axis, from),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_move(
        position(axis, to),
        gpui::MouseButton::Left,
        Default::default(),
    );
    visual.simulate_mouse_up(
        position(axis, to),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
}

// A fixed size below its child's minimum, as a saved size becomes when the UI
// font grows, is laid out at the minimum, and its handle still moves.
#[gpui::test]
fn a_fixed_size_below_its_minimum_is_laid_out_at_the_minimum(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let children = with_sizes(&[
            (AxisSize::Fixed(gpui::px(50.)), 120.),
            (AxisSize::Flex(1.), 80.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children.clone(), bounds.clone(), 600.);
        assert_sizes(&bounds, axis, &[120., 480.]);
        drag(window, cx, axis, 120., 300.);
        assert_sizes(&bounds, axis, &[300., 300.]);
        assert_eq!(children.borrow()[0].size, AxisSize::Fixed(gpui::px(300.)));
        // Dragged back past it, the child stops at its minimum.
        drag(window, cx, axis, 300., 20.);
        assert_sizes(&bounds, axis, &[120., 480.]);

        // The same with the fixed child after a flexible one.
        let children = with_sizes(&[
            (AxisSize::Flex(1.), 80.),
            (AxisSize::Fixed(gpui::px(50.)), 120.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children.clone(), bounds.clone(), 600.);
        assert_sizes(&bounds, axis, &[480., 120.]);
        drag(window, cx, axis, 480., 300.);
        assert_sizes(&bounds, axis, &[300., 300.]);
    }
}
