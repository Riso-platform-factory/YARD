use gpui::{Along, AppContext, IntoElement, ParentElement, Styled};
use riso_ui::{AxisChild, AxisSize};
use std::{cell::RefCell, rc::Rc};

// The minimums these tests rely on: 80 px across and 100 px down.
fn with_minimums(axis: gpui::Axis, sizes: Vec<AxisSize>) -> Vec<AxisChild> {
    let min = match axis {
        gpui::Axis::Horizontal => gpui::px(80.),
        gpui::Axis::Vertical => gpui::px(100.),
    };
    sizes
        .into_iter()
        .map(|size| AxisChild { size, min })
        .collect()
}

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
            "probe",
            self.axis,
            Rc::new(RefCell::new(with_minimums(
                self.axis,
                vec![AxisSize::Flex(1.); self.count],
            ))),
            children,
        ))
    }
}

#[gpui::test]
fn equal_shares_and_dividers(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    for (axis, count, width, height) in [
        (gpui::Axis::Horizontal, 3, 302., 180.),
        (gpui::Axis::Vertical, 2, 210., 181.),
        (gpui::Axis::Horizontal, 1, 302., 180.),
        (gpui::Axis::Horizontal, 0, 302., 180.),
    ] {
        let bounds = Rc::new(RefCell::new(Vec::new()));
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| Probe {
                    axis,
                    count,
                    bounds: bounds.clone(),
                })
            })
            .unwrap()
        });
        let size = gpui::size(gpui::px(width), gpui::px(height));
        cx.simulate_window_resize(window.into(), size);
        let trace = Trace::default();
        tracing::subscriber::with_default(trace.clone(), || {
            gpui::AnyWindowHandle::from(window)
                .update(cx, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                    let bounds = bounds.borrow();
                    assert_eq!(bounds.len(), count);
                    let share = if count == 0 {
                        gpui::px(0.)
                    } else {
                        (size.along(axis) / count as f32).round()
                    };
                    for (index, bounds) in bounds.iter().enumerate() {
                        assert_eq!(bounds.origin.along(axis), share * index as f32);
                        assert_eq!(bounds.size, size.apply_along(axis, |_| share));
                    }
                    let quads = window.painted_quads();
                    assert_eq!(quads.len(), count.saturating_sub(1));
                    for (index, quad) in quads.iter().enumerate() {
                        let expected = gpui::Bounds {
                            origin: gpui::point(gpui::px(0.), gpui::px(0.))
                                .apply_along(axis, |_| share * (index + 1) as f32),
                            size: size.apply_along(axis, |_| gpui::px(1.)),
                        };
                        assert_eq!(quad.bounds, expected.scale(window.scale_factor()));
                        assert_eq!(quad.background, riso_ui::palette(cx).border.into());
                    }
                })
                .unwrap();
        });
        let share = if count == 0 {
            gpui::px(0.)
        } else {
            (size.along(axis) / count as f32).round()
        };
        let child_size = size.apply_along(axis, |_| share);
        let minimum = match axis {
            gpui::Axis::Horizontal => 80.,
            gpui::Axis::Vertical => 100.,
        };
        let expected: Vec<_> = (count > 0)
            .then(|| {
                format!(
                    "TRACE message=child minimums split=probe mins={:?}",
                    vec![minimum as f32; count]
                )
            })
            .into_iter()
            .chain((0..count).map(|index| {
                format!(
                    "TRACE message=axis child size id=probe index={index} width={} height={}",
                    child_size.width, child_size.height
                )
            }))
            .collect();
        assert_eq!(*trace.0.lock().unwrap(), expected);
    }
}
