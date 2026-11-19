use gpui::{
    App, AppContext, Context, Global, InteractiveElement, IntoElement, ParentElement, Render,
    Styled, Subscription, Window, rems,
};
use yard_ui::{Label, palette, v_flex};

gpui::actions!(keys_tour, [Increment, Ping]);

#[derive(Default)]
struct Pings(usize);

impl Global for Pings {}

pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    let case = std::env::var("KEYS_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    match case.as_str() {
        "all" => {
            cx.set_global(Pings::default());
            ext.register(cx, "keys_tour.card", &[], |node, _, _, cx| {
                let node = node.to_string();
                cx.new(|cx| Card {
                    node,
                    count: 0,
                    _pings: cx.observe_global::<Pings>(|_, cx| cx.notify()),
                })
                .into()
            });
            ext.bind_keys(
                cx,
                &[
                    ("ctrl-alt-i", Box::new(Increment), "keys_tour"),
                    ("ctrl-alt-p", Box::new(Ping), "Shell"),
                ],
            );
            ext.on_action::<Ping>(cx, |_, _, cx| {
                cx.default_global::<Pings>().0 += 1;
                tracing::info!("ping");
            });
            ext.place_root(cx, "root", "yard.row");
            for node in ["a", "b"] {
                ext.place(cx, node, "keys_tour.card", "root", "children");
            }
        }
        _ => log::warn!("unknown KEYS_TOUR case {case:?}; known cases: all"),
    }
}

struct Card {
    node: String,
    count: usize,
    _pings: Subscription,
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(node = %self.node, "rendering card");
        let palette = palette(cx);
        let pings = cx.try_global::<Pings>().map_or(0, |pings| pings.0);
        let mut focused = false;
        let mut card = v_flex()
            .debug_selector(|| format!("CARD-{}", self.node))
            .size_full()
            .p(rems(0.5))
            .gap(rems(0.25))
            .bg(palette.background)
            .border_1()
            .border_color(palette.border);
        if let Some(handle) = yard_shell::focus_handle(cx, &self.node) {
            focused = handle.is_focused(window);
            card = card
                .track_focus(&handle)
                .key_context("keys_tour")
                .on_action(cx.listener(|card, _: &Increment, _, cx| {
                    card.count += 1;
                    tracing::info!(node = card.node.as_str(), count = card.count, "incremented");
                    cx.notify();
                }));
        }
        card.child(Label::new(self.node.clone()))
            .child(Label::new(format!("count: {}", self.count)))
            .child(Label::new(format!("pings: {pings}")))
            .children(focused.then(|| Label::new("focused")))
    }
}
