use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use std::{cell::RefCell, rc::Rc};

struct Probe {
    direction: gpui::Axis,
    preferred: [f32; 3],
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let children = (0..3)
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
        gpui::div().size_full().child(riso_ui::axis(
            "rounded-fixed",
            self.direction,
            Rc::new(RefCell::new(
                self.preferred
                    .iter()
                    .map(|size| riso_ui::AxisSize::Fixed(gpui::px(*size)))
                    .collect(),
            )),
            children,
        ))
    }
}

#[gpui::test]
fn the_last_fixed_child_absorbs_rounding_to_fill_the_split(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    for (direction, preferred, length) in [
        (gpui::Axis::Horizontal, [100.4; 3], 301.),
        (gpui::Axis::Vertical, [100.4; 3], 301.),
        (gpui::Axis::Horizontal, [80.6, 80.6, 80.], 241.),
        (gpui::Axis::Vertical, [100.6, 100.6, 100.], 301.),
    ] {
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| Probe {
                    direction,
                    preferred,
                    bounds: bounds.clone(),
                })
            })
            .unwrap()
        });
        cx.simulate_window_resize(
            window.into(),
            gpui::size(gpui::px(400.), gpui::px(400.)).apply_along(direction, |_| gpui::px(length)),
        );
        gpui::AnyWindowHandle::from(window)
            .update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            })
            .unwrap();
        let bounds = bounds.borrow();
        let lengths: Vec<_> = bounds
            .iter()
            .map(|bounds| bounds.size.along(direction))
            .collect();
        assert_eq!(
            lengths.iter().map(|size| f32::from(*size)).sum::<f32>(),
            length
        );
        let minimum = match direction {
            gpui::Axis::Horizontal => 80.,
            gpui::Axis::Vertical => 100.,
        };
        assert!(lengths.iter().all(|size| *size >= gpui::px(minimum)));
        if preferred == [100.4; 3] {
            assert_eq!(lengths, [gpui::px(100.), gpui::px(100.), gpui::px(101.)]);
        }
        assert_eq!(
            bounds[2].origin.along(direction) + lengths[2],
            gpui::px(length)
        );
    }
}
