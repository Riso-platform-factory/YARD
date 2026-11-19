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

fn drag(
    window: gpui::WindowHandle<Probe>,
    cx: &mut gpui::TestAppContext,
    axis: gpui::Axis,
    from: f32,
    to: f32,
) {
    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
    let at = position(axis, from);
    let target = position(axis, to);
    visual.simulate_mouse_move(at, None, Default::default());
    visual.simulate_mouse_down(at, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_move(target, gpui::MouseButton::Left, Default::default());
    visual.simulate_mouse_up(target, gpui::MouseButton::Left, Default::default());
}

#[gpui::test]
fn outer_lines_have_a_full_hit_area_inside_the_split(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        for trailing in [false, true] {
            for inset in [-0.5, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5] {
                let mut values = vec![(AxisSize::Fixed(gpui::px(240.)), true)];
                values.insert(usize::from(!trailing), (AxisSize::Flex(1.), false));
                let sizes = Rc::new(RefCell::new(children(axis, &values)));
                let reports = Rc::new(RefCell::new(Vec::new()));
                let bounds = Rc::new(RefCell::new(Vec::new()));
                let window = open(cx, axis, sizes.clone(), reports, bounds.clone(), 600.);
                drag(
                    window,
                    cx,
                    axis,
                    if trailing { 600. - inset } else { inset },
                    if trailing { 450. } else { 150. },
                );
                assert_eq!(
                    !collapsed(&sizes, usize::from(trailing)),
                    (0. ..8.).contains(&inset),
                    "trailing={trailing}, inset={inset}"
                );
            }
        }
    }
}

#[gpui::test]
fn shown_neighbours_resize_beside_a_hidden_line(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        let values = [
            (AxisSize::Flex(1.), false),
            (AxisSize::Fixed(gpui::px(240.)), true),
            (AxisSize::Flex(1.), false),
        ];
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
        drag(window, cx, axis, 297., 350.);
        draw(window, cx);
        assert!(collapsed(&sizes, 1));
        assert_eq!(painted_len(&bounds, axis, 0), 350.);
        assert!(
            matches!(reports.borrow().last(), Some(Resized::Dragged { snapped_open, .. }) if snapped_open.is_empty())
        );
        drag(window, cx, axis, 351., 500.);
        assert!(
            !collapsed(&sizes, 1),
            "sizes={:?}, reports={:?}",
            sizes.borrow(),
            reports.borrow()
        );
    }
}

#[gpui::test]
fn each_consecutive_line_can_be_grabbed_without_opening_another(cx: &mut gpui::TestAppContext) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        for placement in 0..3 {
            for chosen in 0..3 {
                let mut values = vec![(AxisSize::Fixed(gpui::px(100.)), true); 3];
                if placement != 0 {
                    values.insert(0, (AxisSize::Flex(1.), false));
                }
                if placement != 2 {
                    values.push((AxisSize::Flex(1.), false));
                }
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
                let boundary = [0., 300., 600.][placement];
                let sign = if placement == 2 { -1. } else { 1. };
                drag(
                    window,
                    cx,
                    axis,
                    boundary
                        + sign * ((2 - chosen) as f32 * if placement == 1 { 4. } else { 8. } + 1.),
                    boundary + sign * 150.,
                );
                let selected = chosen + usize::from(placement != 0);
                for (index, child) in sizes.borrow().iter().enumerate() {
                    if values[index].1 {
                        assert_eq!(
                            child.collapsed,
                            index != selected,
                            "placement={placement}, selected={selected}, index={index}"
                        );
                    }
                }
                assert!(
                    matches!(reports.borrow().last(), Some(Resized::Dragged { snapped_open, .. }) if snapped_open == &[selected])
                );
            }
        }
    }
}

#[gpui::test]
fn collapsed_dividers_are_one_pixel_and_visible_inside_both_outer_edges(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        for trailing in [false, true] {
            let mut values = vec![(AxisSize::Fixed(gpui::px(240.)), true)];
            values.insert(usize::from(!trailing), (AxisSize::Flex(1.), false));
            let sizes = Rc::new(RefCell::new(children(axis, &values)));
            let window = open(
                cx,
                axis,
                sizes,
                Rc::new(RefCell::new(Vec::new())),
                Rc::new(RefCell::new(Vec::new())),
                600.,
            );
            let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
            let (quads, scale, border) = visual.update(|window, cx| {
                (
                    window.painted_quads(),
                    window.scale_factor(),
                    yard_ui::palette(cx).border,
                )
            });
            assert!(
                quads.iter().any(|quad| {
                    let bounds = quad.bounds.map(|value| gpui::px(value.as_f32() / scale));
                    quad.background.as_solid() == Some(border)
                        && f32::from(bounds.size.along(axis)) == 1.
                        && f32::from(bounds.origin.along(axis)) == if trailing { 598. } else { 1. }
                }),
                "trailing={trailing}"
            );
        }
    }
}

#[gpui::test]
fn outer_hit_areas_stay_inside_a_split_smaller_than_its_shown_minimum(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(yard_ui::init);
    for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
        for trailing in [false, true] {
            for length in [4., 12.] {
                for inset in [-0.5, 0.5, 1.5, 2.5, 3.5, 4.5, 7.5, 8.5, 12.5] {
                    let mut values = vec![(AxisSize::Fixed(gpui::px(240.)), true)];
                    values.insert(
                        usize::from(!trailing),
                        (AxisSize::Fixed(gpui::px(240.)), false),
                    );
                    let sizes = Rc::new(RefCell::new(children(axis, &values)));
                    let reports = Rc::new(RefCell::new(Vec::new()));
                    let window = open(
                        cx,
                        axis,
                        sizes,
                        reports.clone(),
                        Rc::new(RefCell::new(Vec::new())),
                        length,
                    );
                    let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
                    let at = position(axis, if trailing { length - inset } else { inset });
                    // A double click proves the handle receives the press without
                    // requiring donor room that this constrained split lacks.
                    visual.simulate_mouse_move(at, None, Default::default());
                    visual.simulate_event(gpui::MouseDownEvent {
                        position: at,
                        button: gpui::MouseButton::Left,
                        click_count: 2,
                        modifiers: Default::default(),
                        first_mouse: false,
                    });
                    visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
                    assert_eq!(
                        *reports.borrow(),
                        if (0. ..length.min(8.)).contains(&inset) {
                            vec![Resized::Reset]
                        } else {
                            Vec::new()
                        },
                        "trailing={trailing}, length={length}, inset={inset}"
                    );
                }
            }
        }
    }
}
