use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};
use yard_ui::{AxisChild, AxisSize, Resized};

type RecordedBounds = Vec<(usize, gpui::Bounds<gpui::Pixels>)>;
type BoundsProbe = Rc<RefCell<RecordedBounds>>;

// The minimums these tests rely on: 80 px across and 100 px down, so a child
// snaps once a drag would make it smaller than 40 px across or 50 px down.
fn children(axis: gpui::Axis, sizes: &[(AxisSize, bool)]) -> Vec<AxisChild> {
    let min = match axis {
        gpui::Axis::Horizontal => gpui::px(80.),
        gpui::Axis::Vertical => gpui::px(100.),
    };
    sizes
        .iter()
        .map(|(size, collapsed)| AxisChild {
            size: *size,
            min,
            collapsed: *collapsed,
        })
        .collect()
}

fn minimum(axis: gpui::Axis) -> f32 {
    match axis {
        gpui::Axis::Horizontal => 80.,
        gpui::Axis::Vertical => 100.,
    }
}

fn half_minimum(axis: gpui::Axis) -> f32 {
    minimum(axis) / 2.
}

// The probe records every `on_resized` call, and the bounds each child is
// painted at by index, so that a child that is not painted is seen too.
struct Probe {
    axis: gpui::Axis,
    sizes: Rc<RefCell<Vec<AxisChild>>>,
    reports: Rc<RefCell<Vec<Resized>>>,
    bounds: BoundsProbe,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let children = (0..self.sizes.borrow().len())
            .map(|index| {
                let bounds = self.bounds.clone();
                gpui::canvas(
                    move |value, _, _| bounds.borrow_mut().push((index, value)),
                    |_, _, _, _| {},
                )
                .size_full()
                .into_any_element()
            })
            .collect();
        let reports = self.reports.clone();
        gpui::div().size_full().child(
            yard_ui::axis("split", self.axis, self.sizes.clone(), children)
                .can_collapse(|_, _| true)
                .on_resized(move |resized, _, _| reports.borrow_mut().push(resized)),
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
    reports: Rc<RefCell<Vec<Resized>>>,
    bounds: BoundsProbe,
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
    let size = gpui::size(gpui::px(200.), gpui::px(200.)).apply_along(axis, |_| gpui::px(length));
    cx.simulate_window_resize(window.into(), size);
    draw(window, cx);
    window
}

fn position(axis: gpui::Axis, along: f32) -> gpui::Point<gpui::Pixels> {
    gpui::point(gpui::px(20.), gpui::px(20.)).apply_along(axis, |_| gpui::px(along))
}

// The handle's 4px hitbox is centered on the child's edge, so the edge itself
// is inside it.
fn handle_at(
    bounds: &RefCell<RecordedBounds>,
    axis: gpui::Axis,
    index: usize,
) -> gpui::Point<gpui::Pixels> {
    let bounds = bounds.borrow();
    let (_, child) = bounds
        .iter()
        .find(|(i, _)| *i == index)
        .expect("the child paints");
    child
        .center()
        .apply_along(axis, |_| child.origin.along(axis) + child.size.along(axis))
}

// The collapsed line of a child takes the place where the child would be:
// the leading edge of the child that follows it is inside its hitbox.
fn collapsed_line_at(
    bounds: &RefCell<RecordedBounds>,
    axis: gpui::Axis,
    next: usize,
) -> gpui::Point<gpui::Pixels> {
    let bounds = bounds.borrow();
    let (_, next) = bounds
        .iter()
        .find(|(i, _)| *i == next)
        .expect("the next child paints");
    next.center().apply_along(axis, |_| next.origin.along(axis))
}

// The length a child is painted at along the axis, or 0 when it is not
// painted at all: a collapsed child takes no room but the line.
fn painted_len(bounds: &RefCell<RecordedBounds>, axis: gpui::Axis, index: usize) -> f32 {
    bounds
        .borrow()
        .iter()
        .find(|(i, _)| *i == index)
        .map_or(0., |(_, child)| f32::from(child.size.along(axis)))
}

fn collapsed(sizes: &Rc<RefCell<Vec<AxisChild>>>, index: usize) -> bool {
    sizes.borrow()[index].collapsed
}

// A drag that would make a child beside the handle smaller than half its
// minimum shows it collapsed while the drag goes on, and at the mouse up
// reports the child in `snapped_shut`, its entry in the sizes kept.
#[gpui::test]
fn a_drag_past_half_a_childs_minimum_collapses_it_and_reports_snapped_shut(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        // The child before the handle: dragged below half its minimum it
        // collapses and is reported.
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Fixed(gpui::px(240.)), false),
                (AxisSize::Flex(1.), false),
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
        assert_eq!(painted_len(&bounds, axis, 0), 240.);
        assert_eq!(painted_len(&bounds, axis, 1), 360.);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 30.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        // While the drag goes on the child is shown collapsed, as if it were
        // not there: the other child has the room.
        assert!(collapsed(&sizes, 0));
        draw(window, cx);
        assert!(painted_len(&bounds, axis, 0) <= 4., "{:?}", bounds.borrow());
        assert!(
            painted_len(&bounds, axis, 1) > 590.,
            "{:?}",
            bounds.borrow()
        );
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Fixed(gpui::px(240.)), AxisSize::Flex(1.)],
                snapped_shut: vec![0],
                snapped_open: vec![],
            }]
        );
        // The child stays collapsed, keeping its size entry.
        assert!(collapsed(&sizes, 0));
        assert_eq!(sizes.borrow()[0].size, AxisSize::Fixed(gpui::px(240.)));

        // The child after the handle snaps the same way.
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Flex(1.), false),
                (AxisSize::Fixed(gpui::px(240.)), false),
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
        assert_eq!(painted_len(&bounds, axis, 0), 360.);
        assert_eq!(painted_len(&bounds, axis, 1), 240.);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 590.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 1));
        draw(window, cx);
        assert!(
            painted_len(&bounds, axis, 0) > 590.,
            "{:?}",
            bounds.borrow()
        );
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Flex(1.), AxisSize::Fixed(gpui::px(240.))],
                snapped_shut: vec![1],
                snapped_open: vec![],
            }]
        );
        assert!(collapsed(&sizes, 1));
        assert_eq!(sizes.borrow()[1].size, AxisSize::Fixed(gpui::px(240.)));
    }
}

// Dragging back past half the minimum before the mouse up brings the child
// back, at its minimum when the pointer is still under it, and nothing is
// recorded for the snap.
#[gpui::test]
fn dragging_back_past_half_the_minimum_before_the_mouse_up_does_not_snap(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Fixed(gpui::px(240.)), false),
                (AxisSize::Flex(1.), false),
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
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // Dragged past half the minimum and back to where it started, the
        // child is shown again and nothing is reported at all.
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 30.);
        let back = position(axis, 240.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        visual.simulate_mouse_move(back, gpui::MouseButton::Left, Default::default());
        assert!(
            !collapsed(&sizes, 0),
            "shown again once back past half the minimum"
        );
        visual.simulate_mouse_up(back, gpui::MouseButton::Left, Default::default());
        assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
        // Back only past half the minimum but still under it, the child is
        // shown at its minimum: a plain resize, no snap.
        let at = handle_at(&bounds, axis, 0);
        let target = position(axis, 30.);
        let back = position(axis, minimum(axis) - 20.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        visual.simulate_mouse_move(back, gpui::MouseButton::Left, Default::default());
        assert!(!collapsed(&sizes, 0));
        visual.simulate_mouse_up(back, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Fixed(gpui::px(minimum(axis))), AxisSize::Flex(1.)],
                snapped_shut: vec![],
                snapped_open: vec![],
            }]
        );
        assert!(!collapsed(&sizes, 0));
    }
}

// The line of a collapsed child drags it open: past half its minimum the
// child grows from its minimum with the pointer, and the mouse up reports it
// in `snapped_open` with the size dragged to.
#[gpui::test]
fn a_collapsed_childs_line_dragged_out_reports_snapped_open(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        // A collapsed child at the start of the axis: it takes no room but
        // the line.
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Fixed(gpui::px(240.)), true),
                (AxisSize::Flex(1.), false),
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
        assert!(painted_len(&bounds, axis, 0) <= 4., "{:?}", bounds.borrow());
        assert!(
            painted_len(&bounds, axis, 1) > 590.,
            "{:?}",
            bounds.borrow()
        );
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // Dragging the line out past half the minimum shows the child again,
        // at the size dragged to.
        let at = collapsed_line_at(&bounds, axis, 1);
        let target = position(axis, 150.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        // During the drag the child grows with the pointer as in a normal
        // resize.
        assert!(!collapsed(&sizes, 0));
        draw(window, cx);
        assert!(
            (painted_len(&bounds, axis, 0) - 150.).abs() <= 1.,
            "{:?}",
            bounds.borrow()
        );
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Fixed(gpui::px(150.)), AxisSize::Flex(1.)],
                snapped_shut: vec![],
                snapped_open: vec![0],
            }]
        );
        assert!(!collapsed(&sizes, 0));

        // Out past half the minimum but under it, the child opens at its
        // minimum.
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Fixed(gpui::px(240.)), true),
                (AxisSize::Flex(1.), false),
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
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let at = collapsed_line_at(&bounds, axis, 1);
        let target = position(axis, minimum(axis) - 20.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Fixed(gpui::px(minimum(axis))), AxisSize::Flex(1.)],
                snapped_shut: vec![],
                snapped_open: vec![0],
            }]
        );
        assert!(!collapsed(&sizes, 0));

        // A collapsed child at the end of the axis: its line at the edge
        // drags it open the other way.
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Flex(1.), false),
                (AxisSize::Fixed(gpui::px(240.)), true),
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
        assert!(painted_len(&bounds, axis, 0) > 590.);
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // The collapsed child's line sits at the split's edge; grab it inside
        // its hitbox, as `handle_at` is the window's outer edge.
        let at = position(axis, 599.);
        let target = position(axis, 500.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        assert!(!collapsed(&sizes, 1));
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert_eq!(
            *reports.borrow(),
            [Resized::Dragged {
                sizes: vec![AxisSize::Flex(1.), AxisSize::Fixed(gpui::px(100.))],
                snapped_shut: vec![],
                snapped_open: vec![1],
            }]
        );
        assert!(!collapsed(&sizes, 1));
    }
}

// A drag of a collapsed child's line that ends under half its minimum leaves
// the child hidden and records nothing.
#[gpui::test]
fn a_line_drag_under_half_the_minimum_leaves_the_child_hidden(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let sizes = Rc::new(RefCell::new(children(
            axis,
            &[
                (AxisSize::Fixed(gpui::px(240.)), true),
                (AxisSize::Flex(1.), false),
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
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        // A short drag out leaves the child hidden.
        let at = collapsed_line_at(&bounds, axis, 1);
        let target = position(axis, 20.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
        // At half the minimum itself: only more than half opens it.
        let at = collapsed_line_at(&bounds, axis, 1);
        let target = position(axis, half_minimum(axis));
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
        // Out past half and back under it before releasing leaves it hidden
        // too, and still nothing is recorded.
        let at = collapsed_line_at(&bounds, axis, 1);
        let out = position(axis, 150.);
        let target = position(axis, 20.);
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(out, gpui::MouseButton::Left, Default::default());
        assert!(!collapsed(&sizes, 0));
        visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, 0));
        assert!(reports.borrow().is_empty(), "{:?}", reports.borrow());
        draw(window, cx);
        assert!(painted_len(&bounds, axis, 0) <= 4., "{:?}", bounds.borrow());
    }
}
