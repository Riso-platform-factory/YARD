use super::*;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct RenderedSizes(Arc<Mutex<Vec<String>>>);
impl tracing::field::Visit for RenderedSizes {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "size" {
            self.0.lock().unwrap().push(format!("{value:?}"));
        }
    }
}
impl tracing::Subscriber for RenderedSizes {
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
        if event.metadata().target().starts_with("layout_tour") {
            event.record(&mut self.clone());
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// Alone in this test binary so every card render uses this subscriber.
#[gpui::test]
fn the_card_renders_each_measured_size_and_settles(cx: &mut gpui::TestAppContext) {
    cx.update(riso_ui::init);
    let trace = RenderedSizes::default();
    let notifications = Rc::new(Cell::new(0));
    tracing::subscriber::with_default(trace.clone(), || {
        let window = cx.update(|cx| {
            let ext = riso_shell::dev::Extension::new(cx, "layout_tour").unwrap();
            let notifications = notifications.clone();
            ext.register(
                cx,
                "layout_tour.card",
                &["body"],
                move |node, slots, _, cx| {
                    let card = cx.new(|_| Card {
                        node: node.to_string(),
                        size: Size::default(),
                        slots,
                    });
                    let notifications = notifications.clone();
                    cx.observe(&card, move |_, _| {
                        notifications.set(notifications.get() + 1)
                    })
                    .detach();
                    card.into()
                },
            );
            ext.place_root(cx, "root", "layout_tour.card");
            cx.open_window(gpui::WindowOptions::default(), riso_shell::dev::new_shell)
                .unwrap()
        });
        for dimensions in [(640., 480.), (900., 600.), (400., 300.)] {
            let size = gpui::size(gpui::px(dimensions.0), gpui::px(dimensions.1));
            cx.simulate_window_resize(window.into(), size);
            let window = gpui::AnyWindowHandle::from(window);
            window
                .update(cx, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                })
                .unwrap();
            // Only a scheduled after-frame callback may invalidate the stale label.
            let callbacks = window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap();
            assert!(
                callbacks > 0,
                "changed bounds must schedule the measured label"
            );
            window
                .update(cx, |_, window, cx| {
                    let _ = window.draw(cx);
                })
                .unwrap();
            assert_eq!(
                trace.0.lock().unwrap().last().unwrap(),
                &format!("{} × {}", size.width, size.height)
            );
            let notices = notifications.get();
            assert_eq!(
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap(),
                0,
                "unchanged bounds schedule no further frame"
            );
            window
                .update(cx, |_, window, cx| {
                    let _ = window.draw(cx);
                })
                .unwrap();
            assert_eq!(
                notifications.get(),
                notices,
                "unchanged bounds do not notify"
            );
            assert_eq!(
                window
                    .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                    .unwrap(),
                0,
                "unchanged measurement schedules no frame"
            );
        }
    });
}
