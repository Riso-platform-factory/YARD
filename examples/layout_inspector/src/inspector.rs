use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle,
    SharedString, Styled, Subscription, Window, div, px,
};
use riso_shell::dev as shell;
use riso_ui::{Label, LabelSize, List, ListHeader, ListItem, ScrollArea, TextColor};

/// Contributes the inspector after the layout tour has placed its frame.
pub fn init(cx: &mut App, ext: &shell::Extension) {
    if std::env::var_os("LAYOUT_INSPECTOR").is_none() {
        return;
    }
    let _group = tracing::info_span!("inspector-init").entered();
    if !shell::inspect(cx)
        .iter()
        .any(|node| node.id == "tour_frame")
    {
        log::warn!("layout inspector cannot be placed: no tour_frame");
        return;
    }
    ext.register(cx, "layout_inspector.panel", &[], |_, _, _, cx| {
        cx.new(Panel::new).into()
    });
    ext.place(
        cx,
        "inspector",
        "layout_inspector.panel",
        "tour_frame",
        "children",
    );
}

struct Panel {
    nodes: Vec<shell::NodeInfo>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl Panel {
    fn new(cx: &mut Context<Self>) -> Self {
        let nodes = shell::inspect(cx);
        let weak = cx.weak_entity();
        let subscription = shell::observe_layout(cx, move |cx| {
            let nodes = shell::inspect(cx);
            let _group = tracing::info_span!("inspector-refresh", nodes = nodes.len()).entered();
            let _ = weak.update(cx, |panel, cx| {
                panel.nodes = nodes;
                cx.notify();
            });
        });
        Self {
            nodes,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!(nodes = self.nodes.len(), "rendering inspector");
        let list = List::new()
            .header(
                ListHeader::new("Layout").inset(true).end_slot(
                    div().debug_selector(|| "INSPECTOR-COUNT".into()).child(
                        Label::new(self.nodes.len().to_string())
                            .size(LabelSize::Small)
                            .color(TextColor::Muted),
                    ),
                ),
            )
            .children(self.nodes.iter().map(|node| {
                let origin = match &node.origin {
                    shell::Origin::Default => "default".into(),
                    shell::Origin::NamedLayout(name) => name.to_string(),
                    shell::Origin::UserLayer => "user".into(),
                    shell::Origin::Copy(key) => format!("copy {key}"),
                };
                let id = node.id.clone();
                let depth = node.depth;
                let hidden = node.hidden;
                let detail_color = if hidden {
                    TextColor::Disabled
                } else {
                    TextColor::Muted
                };
                let id_color = if hidden {
                    TextColor::Disabled
                } else {
                    TextColor::Default
                };
                let detail = if node.props.is_null() {
                    node.component.to_string()
                } else {
                    format!("{} {}", node.component, node.props)
                };
                div()
                    .w_full()
                    .min_w_0()
                    .pl(px(node.depth as f32 * 12.))
                    .debug_selector(move || {
                        format!("INSPECTOR-ROW-{id}-depth-{depth}-hidden-{hidden}")
                    })
                    .child(
                        ListItem::new(node.id.clone())
                            .disabled(node.hidden)
                            .start_slot(
                                riso_ui::h_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .debug_selector({
                                                let id = node.id.clone();
                                                move || format!("INSPECTOR-ID-{id}")
                                            })
                                            .child(Label::new(node.id.clone()).color(id_color)),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .child(Label::new(detail).color(detail_color)),
                                    ),
                            )
                            .end_slot(
                                div()
                                    .flex_shrink_0()
                                    .debug_selector({
                                        let id = node.id.clone();
                                        move || format!("INSPECTOR-END-{id}")
                                    })
                                    .child(
                                        Label::new(SharedString::from(format!(
                                            "{} {origin}",
                                            node.extension
                                        )))
                                        .color(detail_color)
                                        .size(LabelSize::Small),
                                    ),
                            ),
                    )
            }));
        ScrollArea::new("layout-inspector", &self.scroll)
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(list)
    }
}
