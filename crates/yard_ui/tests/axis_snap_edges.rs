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
// painted at all: a collapsed child takes no room.
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

// Preserve gesture identity across redraws, including a disappearing handle.
#[gpui::test]
fn snap_and_reversal_survive_redraw_on_both_sides(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    for first in [true, false] {
        let values = if first {
            [
                (AxisSize::Fixed(gpui::px(240.)), false),
                (AxisSize::Flex(1.), false),
            ]
        } else {
            [
                (AxisSize::Flex(1.), false),
                (AxisSize::Fixed(gpui::px(240.)), false),
            ]
        };
        let sizes = Rc::new(RefCell::new(children(axis, &values)));
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
        let at = handle_at(&bounds, axis, 0);
        let shut = position(axis, if first { 30. } else { 590. });
        visual.simulate_mouse_move(at, None, Default::default());
        visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
        visual.simulate_mouse_move(shut, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        visual.simulate_mouse_move(shut, gpui::MouseButton::Left, Default::default());
        assert!(collapsed(&sizes, usize::from(!first)));
        visual.simulate_mouse_move(at, gpui::MouseButton::Left, Default::default());
        draw(window, cx);
        visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
        assert_eq!(*sizes.borrow(), children(axis, &values));
        assert!(reports.borrow().is_empty());
    }
}

#[gpui::test]
fn opening_and_reversal_survive_redraw(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
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
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        position(axis, 150.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_move(
        position(axis, 200.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    assert_eq!(painted_len(&bounds, axis, 0), 200.);
    visual.simulate_mouse_move(
        position(axis, 20.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_up(
        position(axis, 20.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    assert!(collapsed(&sizes, 0));
    assert!(reports.borrow().is_empty());
}

#[gpui::test]
fn fixed_only_trailing_line_opens_at_the_pointer(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(
        axis,
        &[
            (AxisSize::Fixed(gpui::px(240.)), false),
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
    assert_eq!(painted_len(&bounds, axis, 0), 600.);
    assert_eq!(painted_len(&bounds, axis, 1), 0.);
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let at = position(axis, 599.);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        position(axis, 500.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_up(
        position(axis, 500.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    assert_eq!(painted_len(&bounds, axis, 0), 500.);
    assert_eq!(painted_len(&bounds, axis, 1), 100.);
    assert!(
        matches!(&reports.borrow()[0], Resized::Dragged { snapped_open, .. } if snapped_open == &[1])
    );
}

#[gpui::test]
fn consecutive_hidden_lines_share_a_boundary_without_taking_room(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    for trailing in [false, true] {
        let mut values = vec![(AxisSize::Fixed(gpui::px(240.)), true); 3];
        if trailing {
            values.insert(0, (AxisSize::Flex(1.), false));
        } else {
            values.push((AxisSize::Flex(1.), false));
        }
        let sizes = Rc::new(RefCell::new(children(axis, &values)));
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let reports = Rc::new(RefCell::new(Vec::new()));
        let window = open(
            cx,
            axis,
            sizes.clone(),
            reports.clone(),
            bounds.clone(),
            600.,
        );
        assert_eq!(
            painted_len(&bounds, axis, if trailing { 0 } else { 3 }),
            600.
        );
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        for remaining in (1..=3).rev() {
            let chosen = sizes
                .borrow()
                .iter()
                .rposition(|child| child.collapsed)
                .unwrap();
            let boundary: f32 = (0..chosen)
                .map(|index| painted_len(&bounds, axis, index))
                .sum();
            let forward = sizes.borrow()[chosen + 1..]
                .iter()
                .any(|child| !child.collapsed);
            let at = position(axis, boundary.clamp(1., 599.));
            let target = position(
                axis,
                boundary
                    + if forward {
                        remaining as f32 * 80.
                    } else {
                        -(remaining as f32) * 80.
                    },
            );
            visual.simulate_mouse_move(at, None, Default::default());
            visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
            visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
            draw(window, cx);
            visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
            assert!(
                !collapsed(&sizes, chosen),
                "trailing={trailing}, remaining={remaining}, chosen={chosen}, sizes={:?}",
                sizes.borrow()
            );
            assert_eq!(
                sizes
                    .borrow()
                    .iter()
                    .filter(|child| child.collapsed)
                    .count(),
                remaining - 1
            );
            assert!(
                matches!(reports.borrow().last().unwrap(), Resized::Dragged { snapped_open, .. } if snapped_open == &[chosen])
            );
        }
    }
}

#[gpui::test]
fn a_single_hidden_child_can_be_opened(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(axis, &[(AxisSize::Flex(1.), true)])));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let reports = Rc::new(RefCell::new(Vec::new()));
    let window = open(
        cx,
        axis,
        sizes.clone(),
        reports.clone(),
        bounds.clone(),
        600.,
    );
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let at = position(axis, 1.);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(
        position(axis, 200.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    draw(window, cx);
    visual.simulate_mouse_up(
        position(axis, 200.),
        gpui::MouseButton::Left,
        Default::default(),
    );
    assert!(!collapsed(&sizes, 0));
    assert_eq!(painted_len(&bounds, axis, 0), 600.);
    assert!(
        matches!(&reports.borrow()[0], Resized::Dragged { snapped_open, .. } if snapped_open == &[0])
    );
}

#[gpui::test]
fn a_middle_hidden_child_opens_with_its_divider_at_the_pointer(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(
        axis,
        &[
            (AxisSize::Flex(1.), false),
            (AxisSize::Fixed(gpui::px(240.)), true),
            (AxisSize::Flex(1.), false),
        ],
    )));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let reports = Rc::new(RefCell::new(Vec::new()));
    let window = open(
        cx,
        axis,
        sizes.clone(),
        reports.clone(),
        bounds.clone(),
        600.,
    );
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let at = collapsed_line_at(&bounds, axis, 2);
    let target = position(axis, 450.);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(!collapsed(&sizes, 1));
    assert_eq!(handle_at(&bounds, axis, 1).x, target.x);
    assert_eq!(painted_len(&bounds, axis, 1), 150.);
}

#[gpui::test]
fn a_flexible_snap_keeps_the_hidden_share_and_redistributes_room(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(
        axis,
        &[(AxisSize::Flex(1.), false); 3],
    )));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let reports = Rc::new(RefCell::new(Vec::new()));
    let window = open(
        cx,
        axis,
        sizes.clone(),
        reports.clone(),
        bounds.clone(),
        600.,
    );
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let at = handle_at(&bounds, axis, 1);
    let target = position(axis, 230.);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    draw(window, cx);
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
    assert!(collapsed(&sizes, 1));
    assert_eq!(sizes.borrow()[1].size, AxisSize::Flex(1.));
    assert!(painted_len(&bounds, axis, 2) > 390.);
    assert!(
        matches!(&reports.borrow()[0], Resized::Dragged { snapped_shut, .. } if snapped_shut == &[1])
    );
}

#[gpui::test]
fn fixed_rounding_with_a_trailing_line_fills_the_split(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(
        axis,
        &[
            (AxisSize::Fixed(gpui::px(100.6)), false),
            (AxisSize::Fixed(gpui::px(100.6)), false),
            (AxisSize::Fixed(gpui::px(240.)), true),
        ],
    )));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let reports = Rc::new(RefCell::new(Vec::new()));
    open(cx, axis, sizes, reports, bounds.clone(), 203.);
    assert_eq!(
        painted_len(&bounds, axis, 0) + painted_len(&bounds, axis, 1),
        203.
    );
    assert!(painted_len(&bounds, axis, 0) >= 80.);
    assert!(painted_len(&bounds, axis, 1) >= 80.);
}

#[gpui::test]
fn collapsed_lines_never_give_shown_children_negative_lengths(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    let axis = gpui::Axis::Horizontal;
    let sizes = Rc::new(RefCell::new(children(
        axis,
        &[
            (AxisSize::Fixed(gpui::px(240.)), true),
            (AxisSize::Fixed(gpui::px(240.)), true),
            (AxisSize::Flex(1.), false),
        ],
    )));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    open(
        cx,
        axis,
        sizes,
        Rc::new(RefCell::new(Vec::new())),
        bounds.clone(),
        4.,
    );
    assert_eq!(painted_len(&bounds, axis, 2), 4.);
}
