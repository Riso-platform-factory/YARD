use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use riso_ui::AxisSize;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Trace(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        write!(&mut self.0, " {}={value:?}", field.name()).unwrap();
    }
}
impl tracing::Subscriber for Trace {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if event.metadata().target() == "riso_ui::axis" {
            let mut fields = Fields(event.metadata().level().to_string());
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

struct Probe {
    axis: gpui::Axis,
    count: usize,
    shares: Rc<RefCell<Vec<AxisSize>>>,
    bounds: Rc<RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
}

impl gpui::Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        self.bounds.borrow_mut().clear();
        let children = (0..self.count)
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
            "split",
            self.axis,
            self.shares.clone(),
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

// Alone in this binary: every event and render reaching the captured lines has
// the same thread-local subscriber.
#[gpui::test]
fn handles_resize_reset_and_log(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    let trace = Trace::default();
    tracing::subscriber::with_default(trace.clone(), || {
        for axis in [gpui::Axis::Horizontal, gpui::Axis::Vertical] {
            let mut shares = Rc::new(RefCell::new(vec![AxisSize::Flex(1.); 3]));
            let bounds = Rc::new(RefCell::new(Vec::new()));
            let window = cx.update(|cx| {
                cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                    cx.new(|_| Probe {
                        axis,
                        count: 3,
                        shares: shares.clone(),
                        bounds: bounds.clone(),
                    })
                })
                .unwrap()
            });
            let initial =
                gpui::size(gpui::px(180.), gpui::px(180.)).apply_along(axis, |_| gpui::px(600.));
            cx.simulate_window_resize(window.into(), initial);
            draw(window, cx);
            assert_eq!(*shares.borrow(), [AxisSize::Flex(1.); 3]);
            let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
            // Three pixels from the divider is outside the centered 4px hitbox.
            visual.simulate_mouse_move(position(axis, 197.), None, Default::default());
            visual.simulate_mouse_down(
                position(axis, 197.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_move(
                position(axis, 230.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_up(
                position(axis, 230.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            assert_eq!(*shares.borrow(), [AxisSize::Flex(1.); 3]);
            visual.simulate_mouse_move(position(axis, 200.), None, Default::default());
            visual.simulate_mouse_down(
                position(axis, 200.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_move(
                position(axis, 240.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, cx);
            assert_sizes(&bounds, axis, &[240., 160., 200.]);
            // Drag survives a render, and the neighbours stop at their axis minimum.
            visual.simulate_mouse_move(
                position(axis, 400.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, cx);
            let minimum = if axis == gpui::Axis::Horizontal {
                80.
            } else {
                100.
            };
            assert_sizes(&bounds, axis, &[400., minimum, 200. - minimum]);
            visual.simulate_mouse_move(
                position(axis, 1000.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, cx);
            assert_sizes(&bounds, axis, &[600. - minimum * 2., minimum, minimum]);
            visual.simulate_mouse_up(
                position(axis, 1000.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            let retained = shares.borrow().clone();
            visual.simulate_mouse_move(position(axis, 250.), None, Default::default());
            draw(window, cx);
            assert_eq!(*shares.borrow(), retained, "mouse up ends dragging");
            cx.simulate_window_resize(window.into(), initial.map(|value| value * 2.));
            draw(window, cx);
            assert_eq!(*shares.borrow(), retained, "window resize preserves shares");
            assert_sizes(
                &bounds,
                axis,
                &[(600. - minimum * 2.) * 2., minimum * 2., minimum * 2.],
            );
            let divider = bounds.borrow()[0].size.along(axis);
            let at = position(axis, divider.into());
            visual.simulate_mouse_move(at, None, Default::default());
            visual.simulate_event(gpui::MouseDownEvent {
                position: at,
                button: gpui::MouseButton::Left,
                click_count: 2,
                modifiers: Default::default(),
                first_mouse: false,
            });
            draw(window, cx);
            assert_eq!(*shares.borrow(), [AxisSize::Flex(1.); 3]);
            assert_sizes(&bounds, axis, &[400., 400., 400.]);
            visual.simulate_mouse_up(at, gpui::MouseButton::Left, Default::default());
            // The second handle pushes predecessors when dragged backwards.
            visual.simulate_mouse_move(position(axis, 800.), None, Default::default());
            visual.simulate_mouse_down(
                position(axis, 800.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_move(
                position(axis, 100.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, cx);
            assert_sizes(&bounds, axis, &[minimum, minimum, 1200. - minimum * 2.]);
            visual.simulate_mouse_up(
                position(axis, 100.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            // Changing the child set supplies matching shares without resetting the previous vector.
            let previous = shares.clone();
            let retained = previous.borrow().clone();
            for count in [1, 0, 3] {
                shares = Rc::new(RefCell::new(vec![AxisSize::Flex(1.); count]));
                window
                    .update(cx, |probe, _, _| {
                        probe.count = count;
                        probe.shares = shares.clone();
                    })
                    .unwrap();
                draw(window, cx);
            }
            assert_eq!(*previous.borrow(), retained);
            // A split made smaller than its minimums cannot be resized.
            cx.simulate_window_resize(window.into(), initial.apply_along(axis, |_| gpui::px(180.)));
            draw(window, cx);
            visual.simulate_mouse_move(position(axis, 60.), None, Default::default());
            visual.simulate_mouse_down(
                position(axis, 60.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            visual.simulate_mouse_move(
                position(axis, 90.),
                gpui::MouseButton::Left,
                Default::default(),
            );
            draw(window, cx);
            assert_eq!(*shares.borrow(), [AxisSize::Flex(1.); 3]);
            visual.simulate_mouse_up(
                position(axis, 90.),
                gpui::MouseButton::Left,
                Default::default(),
            );
        }
    });
    let lines = trace.0.lock().unwrap();
    let resize: Vec<_> = lines
        .iter()
        .filter(|line| !line.contains("axis child size"))
        .collect();
    assert_eq!(
        resize
            .iter()
            .filter(|line| line.contains("resize started"))
            .count(),
        8
    );
    assert_eq!(
        resize
            .iter()
            .filter(|line| line.contains("resize ended"))
            .count(),
        8
    );
    assert_eq!(
        resize
            .iter()
            .filter(|line| line.contains("shares reset"))
            .count(),
        2
    );
    assert_eq!(
        resize
            .iter()
            .filter(|line| line.contains("shares changed"))
            .count(),
        7
    );
    for line in resize {
        assert!(line.contains("id=\"split\""), "{line}");
        if line.contains("shares changed") {
            assert!(
                line.starts_with("TRACE ") && line.contains("shares=["),
                "{line}"
            );
        } else {
            assert!(line.starts_with("DEBUG "), "{line}");
        }
        if line.contains("resize started") {
            assert!(
                (line.contains("handle=0") || line.contains("handle=1")),
                "{line}"
            );
        }
        if line.contains("resize ended") {
            assert!(line.contains("shares=["), "{line}");
        }
    }
}
