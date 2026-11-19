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

// A drag pushes the children it shrinks only as far as each child's own
// minimum: the neighbours stop at theirs, and the children before the dragged
// one give what they have, down to theirs.
#[gpui::test]
fn a_drag_stops_at_each_childs_own_minimum(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let children = with_sizes(&[
            (AxisSize::Flex(1.), 40.),
            (AxisSize::Flex(1.), 120.),
            (AxisSize::Flex(1.), 60.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children, bounds.clone(), 600.);
        assert_sizes(&bounds, axis, &[200., 200., 200.]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // The first handle sits at the end of the first child's 200 pixels.
        let at = position(axis, 200.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            position(axis, 590.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_up(
            position(axis, 590.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        draw(window, cx);
        // The second child stops at its own 120, the third at its own 60.
        assert_sizes(&bounds, axis, &[420., 120., 60.]);
        // Dragging the second handle backward pushes the first child to its
        // own 40: the second child has nothing left to give.
        let at = position(axis, 540.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            position(axis, 100.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_up(
            position(axis, 100.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        draw(window, cx);
        assert_sizes(&bounds, axis, &[40., 120., 440.]);
    }
}

// A child whose minimum is larger stops the same drag earlier.
#[gpui::test]
fn a_child_with_a_larger_minimum_stops_the_drag_earlier(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let children = with_sizes(&[
            (AxisSize::Flex(1.), 40.),
            (AxisSize::Flex(1.), 160.),
            (AxisSize::Flex(1.), 40.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children, bounds.clone(), 600.);
        assert_sizes(&bounds, axis, &[200., 200., 200.]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let at = position(axis, 200.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            position(axis, 590.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        visual.simulate_mouse_up(
            position(axis, 590.),
            gpui::MouseButton::Left,
            Default::default(),
        );
        draw(window, cx);
        // The middle child's 160 stops the drag where a smaller one would not.
        assert_sizes(&bounds, axis, &[400., 160., 40.]);
    }
}

// Shrinking the axis gives the flexible children their own minimums first,
// then shrinks the fixed children in proportion, never below theirs. Grown
// back, the fixed children have their sizes again.
#[gpui::test]
fn shrinking_the_axis_respects_each_minimum(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        // Two flexible children sit at their own minimums, not a shared one.
        let children = with_sizes(&[
            (AxisSize::Fixed(gpui::px(300.)), 100.),
            (AxisSize::Flex(1.), 40.),
            (AxisSize::Flex(1.), 140.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children, bounds.clone(), 700.);
        assert_sizes(&bounds, axis, &[300., 200., 200.]);
        resize(window, cx, axis, 400.);
        assert_sizes(&bounds, axis, &[220., 40., 140.]);
        // Each fixed child stops at its own minimum.
        let children = with_sizes(&[
            (AxisSize::Fixed(gpui::px(300.)), 160.),
            (AxisSize::Fixed(gpui::px(200.)), 80.),
            (AxisSize::Flex(1.), 50.),
        ]);
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(cx, axis, children, bounds.clone(), 800.);
        assert_sizes(&bounds, axis, &[300., 200., 300.]);
        resize(window, cx, axis, 400.);
        assert_sizes(&bounds, axis, &[210., 140., 50.]);
        resize(window, cx, axis, 250.);
        assert_sizes(&bounds, axis, &[160., 80., 50.]);
        resize(window, cx, axis, 800.);
        assert_sizes(&bounds, axis, &[300., 200., 300.]);
    }
}
