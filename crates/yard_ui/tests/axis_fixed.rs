use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};
use yard_ui::{AxisChild, AxisSize, Resized};

// The minimums these tests rely on: 80 px across and 100 px down.
fn with_minimums(axis: gpui::Axis, sizes: Vec<AxisSize>) -> Vec<AxisChild> {
    let min = match axis {
        gpui::Axis::Horizontal => gpui::px(80.),
        gpui::Axis::Vertical => gpui::px(100.),
    };
    sizes
        .into_iter()
        .map(|size| AxisChild {
            size,
            min,
            collapsed: false,
        })
        .collect()
}

// What `on_resized` reports, read without relying on `AxisSize`'s own traits.
#[derive(Debug, PartialEq)]
enum Seen {
    Dragged(Vec<SeenSize>),
    Reset,
}

#[derive(Debug, PartialEq)]
enum SeenSize {
    Fixed(f32),
    Flex(f32),
}

struct Probe {
    axis: gpui::Axis,
    sizes: Rc<RefCell<Vec<AxisChild>>>,
    reports: Rc<RefCell<Vec<Seen>>>,
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let count = self.sizes.borrow().len();
        let children = (0..count)
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
        let reports = self.reports.clone();
        gpui::div().size_full().child(
            yard_ui::axis("split", self.axis, self.sizes.clone(), children).on_resized(
                move |resized, _, _| {
                    reports.borrow_mut().push(match resized {
                        Resized::Dragged { sizes, .. } => Seen::Dragged(
                            sizes
                                .iter()
                                .map(|size| match size {
                                    AxisSize::Fixed(pixels) => SeenSize::Fixed(f32::from(pixels)),
                                    AxisSize::Flex(share) => SeenSize::Flex(*share),
                                })
                                .collect(),
                        ),
                        Resized::Reset => Seen::Reset,
                    });
                },
            ),
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

fn open(
    cx: &mut gpui::TestAppContext,
    axis: gpui::Axis,
    sizes: Rc<RefCell<Vec<AxisChild>>>,
    reports: Rc<RefCell<Vec<Seen>>>,
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
    length: f32,
) -> gpui::WindowHandle<Probe> {
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), |_, cx| {
            cx.new(|_| Probe {
                axis,
                sizes,
                reports,
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

// The handle's 4px hitbox is centered on the child's edge, so the edge itself
// is inside it.
fn handle_at(
    bounds: &RefCell<Vec<gpui::Bounds<gpui::Pixels>>>,
    axis: gpui::Axis,
    index: usize,
) -> gpui::Point<gpui::Pixels> {
    let bounds = bounds.borrow();
    let child = bounds[index];
    child
        .center()
        .apply_along(axis, |_| child.origin.along(axis) + child.size.along(axis))
}

fn fixed_at(sizes: &Rc<RefCell<Vec<AxisChild>>>, index: usize) -> Option<f32> {
    match &sizes.borrow()[index].size {
        AxisSize::Fixed(pixels) => Some(f32::from(pixels)),
        AxisSize::Flex(_) => None,
    }
}

fn flex_at(sizes: &Rc<RefCell<Vec<AxisChild>>>, index: usize) -> Option<f32> {
    match &sizes.borrow()[index].size {
        AxisSize::Flex(share) => Some(*share),
        AxisSize::Fixed(_) => None,
    }
}

// A fixed child keeps its size when the axis grows and shrinks; the flexible
// children take the growth and the shrinkage, down to their minimum.
#[gpui::test]
fn a_fixed_child_keeps_its_size_as_the_axis_grows(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![
                AxisSize::Flex(1.),
                AxisSize::Fixed(gpui::px(240.)),
                AxisSize::Flex(1.),
            ],
        )));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            Rc::new(RefCell::new(Vec::new())),
            bounds.clone(),
            840.,
        );
        assert_sizes(&bounds, axis, &[300., 240., 300.]);
        // A wider window grows only the flexible children.
        resize(window, cx, axis, 1200.);
        assert_sizes(&bounds, axis, &[480., 240., 480.]);
        // A narrower one that still has room does the same.
        resize(window, cx, axis, 700.);
        assert_sizes(&bounds, axis, &[230., 240., 230.]);
    }
}

// When the axis shrinks, the flexible children shrink to their minimum first;
// then the fixed children shrink in proportion to their sizes and never below
// the minimum. Grown back, the fixed children have their sizes again.
#[gpui::test]
fn the_flexible_children_shrink_first_then_the_fixed_shrink_in_proportion(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(yard_ui::init);
    for (axis, steps) in [
        (
            gpui::Axis::Horizontal,
            [
                (380., [80., 180., 120.]),
                (300., [80., 132., 88.]),
                (200., [80., 80., 80.]),
            ],
        ),
        (
            gpui::Axis::Vertical,
            [
                (420., [100., 192., 128.]),
                (380., [100., 168., 112.]),
                (260., [100., 100., 100.]),
            ],
        ),
    ] {
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![
                AxisSize::Flex(1.),
                AxisSize::Fixed(gpui::px(240.)),
                AxisSize::Fixed(gpui::px(160.)),
            ],
        )));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            Rc::new(RefCell::new(Vec::new())),
            bounds.clone(),
            800.,
        );
        assert_sizes(&bounds, axis, &[400., 240., 160.]);
        for (length, expected) in steps {
            resize(window, cx, axis, length);
            assert_sizes(&bounds, axis, &expected);
            // The recorded sizes do not change while the axis is too small.
            assert_eq!(fixed_at(&sizes, 1), Some(240.));
            assert_eq!(fixed_at(&sizes, 2), Some(160.));
        }
        // Grown back, the fixed children have their sizes again.
        resize(window, cx, axis, 800.);
        assert_sizes(&bounds, axis, &[400., 240., 160.]);
    }
}

// Between two fixed children, one grows and the other shrinks by the distance
// dragged. Between a fixed and a flexible child, the fixed child's size
// changes by the distance dragged and the flexible child takes the rest. A
// size never goes below the minimum.
#[gpui::test]
fn dragging_handles_between_fixed_and_flexible_children(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let minimum = if axis == gpui::Axis::Horizontal {
            80.
        } else {
            100.
        };
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![
                AxisSize::Fixed(gpui::px(240.)),
                AxisSize::Fixed(gpui::px(160.)),
                AxisSize::Flex(1.),
            ],
        )));
        let reports = Rc::new(RefCell::new(Vec::new()));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            reports.clone(),
            bounds.clone(),
            600.,
        );
        assert_sizes(&bounds, axis, &[240., 160., 200.]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // Between two fixed children: the first grows by the distance dragged
        // and the second shrinks by the same; the flexible child is untouched.
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 300.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        assert_sizes(&bounds, axis, &[300., 100., 200.]);
        assert_eq!(fixed_at(&sizes, 0), Some(300.));
        assert_eq!(fixed_at(&sizes, 1), Some(100.));
        assert_eq!(flex_at(&sizes, 2), Some(1.));
        // Between a fixed and a flexible child: the fixed child's size changes
        // by the distance dragged and the flexible child takes the rest.
        let at = handle_at(&bounds, axis, 1);
        let target = position(axis, 450.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        assert_sizes(&bounds, axis, &[300., 150., 150.]);
        assert_eq!(fixed_at(&sizes, 1), Some(150.));
        // Dragging the fixed child to and past its minimum leaves it at the
        // minimum, and the flexible child takes the rest.
        let at = handle_at(&bounds, axis, 1);
        let stop = position(axis, 300. + minimum);
        let beyond = position(axis, 300.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(stop, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(beyond, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(beyond, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        assert_sizes(&bounds, axis, &[300., minimum, 600. - 300. - minimum]);
        assert_eq!(fixed_at(&sizes, 1), Some(minimum));
        // Three drags that changed the sizes: three Dragged reports.
        assert_eq!(reports.borrow().len(), 3);
        assert!(
            reports
                .borrow()
                .iter()
                .all(|report| matches!(report, Seen::Dragged(_))),
            "{:?}",
            reports.borrow()
        );
        // A double click reports Reset: the flexible share is reset and the
        // fixed children stay fixed.
        let at = handle_at(&bounds, axis, 1);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_event(gpui::MouseDownEvent {
            position: at,
            button: gpui::MouseButton::Left,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
        assert_eq!(reports.borrow().len(), 4);
        assert!(
            matches!(reports.borrow().last(), Some(Seen::Reset)),
            "{:?}",
            reports.borrow()
        );
        assert_eq!(fixed_at(&sizes, 0), Some(300.));
        assert_eq!(fixed_at(&sizes, 1), Some(minimum));
        assert_eq!(flex_at(&sizes, 2), Some(1.));
    }
}

// `on_resized` reports `Resized::Dragged` with the sizes as they are after a
// drag, even a drag that happens to end on equal shares, and `Resized::Reset`
// after a double click.
#[gpui::test]
fn a_drag_that_ends_on_equal_shares_reports_dragged_not_reset(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![AxisSize::Flex(2.), AxisSize::Flex(1.)],
        )));
        let reports = Rc::new(RefCell::new(Vec::new()));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            reports.clone(),
            bounds.clone(),
            600.,
        );
        assert_sizes(&bounds, axis, &[400., 200.]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // The drag ends on equal shares: it is still a drag, not a reset.
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 300.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Seen::Dragged(vec![
                SeenSize::Flex(1.5),
                SeenSize::Flex(1.5)
            ])]
        );
        // A double click reports Reset and the shares are equal again.
        let at = handle_at(&bounds, axis, 0);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_event(gpui::MouseDownEvent {
            position: at,
            button: gpui::MouseButton::Left,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
        assert_eq!(reports.borrow().len(), 2);
        assert!(
            matches!(reports.borrow().last(), Some(Seen::Reset)),
            "{:?}",
            reports.borrow()
        );
        assert_eq!(flex_at(&sizes, 0), Some(1.));
        assert_eq!(flex_at(&sizes, 1), Some(1.));
    }
}

// A caller that passes only Flex sizes sees the layout of shares: the
// children share the length by their shares, resizing the window keeps the
// fractions, and a double click makes the shares equal again.
#[gpui::test]
fn a_caller_that_passes_only_flex_sees_todays_layout(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![AxisSize::Flex(1.), AxisSize::Flex(1.), AxisSize::Flex(1.)],
        )));
        let reports = Rc::new(RefCell::new(Vec::new()));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            reports.clone(),
            bounds.clone(),
            600.,
        );
        assert_sizes(&bounds, axis, &[200., 200., 200.]);
        // Shares need not be equal: a caller may pass its own proportions.
        *sizes.borrow_mut() = with_minimums(
            axis,
            vec![AxisSize::Flex(3.), AxisSize::Flex(1.), AxisSize::Flex(1.)],
        );
        draw(window, cx);
        assert_sizes(&bounds, axis, &[360., 120., 120.]);
        // Resizing the window keeps the fractions.
        resize(window, cx, axis, 1200.);
        assert_sizes(&bounds, axis, &[720., 240., 240.]);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // A drag between two flexible children changes their shares.
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 800.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        assert_sizes(&bounds, axis, &[800., 160., 240.]);
        assert_eq!(reports.borrow().len(), 1);
        assert!(
            matches!(reports.borrow()[0], Seen::Dragged(_)),
            "{:?}",
            reports.borrow()
        );
        // A double click makes the shares equal again.
        let at = handle_at(&bounds, axis, 0);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_event(gpui::MouseDownEvent {
            position: at,
            button: gpui::MouseButton::Left,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
        assert_eq!(reports.borrow().len(), 2);
        assert!(
            matches!(reports.borrow().last(), Some(Seen::Reset)),
            "{:?}",
            reports.borrow()
        );
        assert_eq!(flex_at(&sizes, 0), Some(1.));
        assert_eq!(flex_at(&sizes, 1), Some(1.));
        assert_eq!(flex_at(&sizes, 2), Some(1.));
        draw(window, cx);
        assert_sizes(&bounds, axis, &[400., 400., 400.]);
    }
}

// With only fixed children, the last shown child gets what is left or
// missing, so the split is always filled.
#[gpui::test]
fn an_axis_of_fixed_children_gives_the_last_child_what_is_left(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(with_minimums(
            axis,
            vec![
                AxisSize::Fixed(gpui::px(200.)),
                AxisSize::Fixed(gpui::px(300.)),
            ],
        )));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            Rc::new(RefCell::new(Vec::new())),
            bounds.clone(),
            800.,
        );
        // The last child takes what is left over the fixed sizes.
        assert_sizes(&bounds, axis, &[200., 600.]);
        resize(window, cx, axis, 500.);
        assert_sizes(&bounds, axis, &[200., 300.]);
        // Too small for the sizes: they shrink in proportion to their sizes.
        resize(window, cx, axis, 400.);
        assert_sizes(&bounds, axis, &[160., 240.]);
        // Grown again, the last child takes what is left again.
        resize(window, cx, axis, 800.);
        assert_sizes(&bounds, axis, &[200., 600.]);
        // The recorded sizes never changed.
        assert_eq!(fixed_at(&sizes, 0), Some(200.));
        assert_eq!(fixed_at(&sizes, 1), Some(300.));
    }
}
