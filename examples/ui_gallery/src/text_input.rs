use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, rems,
};
use yard_ui::{Divider, Label, TextColor, TextInput, TextInputEvent, h_flex, v_flex};

pub(super) fn register(cx: &mut App, ext: &yard_shell::Extension) {
    ext.register(cx, "ui_gallery.text_input", &[], |_, _, window, cx| {
        cx.new(|cx| TextInputPage::new(window, cx)).into()
    });
}

struct TextInputPage {
    inputs: [Entity<TextInput>; 2],
    last_events: [String; 2],
    _subscriptions: Vec<Subscription>,
}

impl TextInputPage {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs = [
            cx.new(|cx| TextInput::new(window, cx).placeholder("Search…")),
            cx.new(|cx| {
                let mut input = TextInput::new(window, cx);
                input.set_text("hello world", cx);
                input
            }),
        ];
        let subscriptions = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                cx.subscribe(input, move |this, input, event, cx| {
                    let event = match event {
                        TextInputEvent::Changed => "changed",
                        TextInputEvent::Confirmed => "confirmed",
                    };
                    this.last_events[index] = format!("{event}: {}", input.read(cx).text());
                    cx.notify();
                })
            })
            .collect();
        Self {
            inputs,
            last_events: Default::default(),
            _subscriptions: subscriptions,
        }
    }
}

impl Render for TextInputPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(page = "text-input", "rendering page");
        v_flex()
            .size_full()
            .p(rems(1.))
            .gap(rems(0.75))
            .bg(yard_ui::palette(cx).background)
            .children(self.inputs.iter().enumerate().map(|(index, input)| {
                v_flex()
                    .w_full()
                    .child(
                        v_flex()
                            .child(
                                h_flex()
                                    .h_9()
                                    .px_2p5()
                                    .flex_none()
                                    .overflow_hidden()
                                    .child(div().flex_1().child(input.clone())),
                            )
                            .child(Divider::horizontal()),
                    )
                    .child(Label::new(self.last_events[index].clone()).color(TextColor::Muted))
            }))
    }
}
