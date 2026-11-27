use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::{cell::RefCell, rc::Rc};
use yard_shell::{Extension, Slots};
use yard_ui::{Button, Label, palette, v_flex};

gpui::actions!(toggle_tour, [Count]);

#[derive(Default)]
struct State {
    count: usize,
    url: Option<String>,
}

pub fn host(cx: &mut App, ext: &Extension) {
    if std::env::var("TOGGLE_TOUR").as_deref() != Ok("on") {
        return;
    }
    let _group = tracing::info_span!("toggle-host-init").entered();
    let host = ext.clone();
    ext.register(cx, "toggle_host.note", &[], move |_, _, _, cx| {
        cx.new(|_| Note(host.clone())).into()
    });
    let host = ext.clone();
    ext.register(cx, "toggle_host.aside", &[], move |_, _, _, cx| {
        cx.new(|_| Aside(host.clone())).into()
    });
    ext.place_root(cx, "toggle-row", "yard.row");
    ext.place(
        cx,
        "toggle-note",
        "toggle_host.note",
        "toggle-row",
        "children",
    );
    ext.place(
        cx,
        "toggle-aside",
        "toggle_host.aside",
        "toggle-row",
        "children",
    );
    tracing::debug!("registered the host");
}

pub fn init(cx: &mut App, ext: &Extension) {
    if std::env::var("TOGGLE_TOUR").as_deref() != Ok("on") {
        return;
    }
    let _group = tracing::info_span!("toggle-tour-init").entered();
    let state = Rc::new(RefCell::new(State::default()));
    let count_state = state.clone();
    ext.bind_keys(cx, &[("ctrl-alt-c", Box::new(Count), "Shell")]);
    ext.on_action::<Count>(cx, move |_, _, cx| {
        let _group = tracing::info_span!("toggle-tour-count").entered();
        let mut state = count_state.borrow_mut();
        state.count += 1;
        tracing::debug!(count = state.count, "counted");
        cx.refresh_windows();
    });
    let url_state = state.clone();
    ext.on_open_url(cx, move |url, cx| {
        let _group = tracing::info_span!("toggle-tour-url").entered();
        url_state.borrow_mut().url = Some(url.to_owned());
        tracing::debug!("updated the card URL");
        cx.refresh_windows();
    });

    ext.register(cx, "toggle_tour.card", &["body"], move |_, slots, _, cx| {
        cx.new(|_| Card {
            slots,
            state: state.clone(),
        })
        .into()
    });
    ext.place(
        cx,
        "toggle-card",
        "toggle_tour.card",
        "toggle-row",
        "children",
    );
    tracing::debug!("registered the tour");
}

struct Card {
    slots: Slots,
    state: Rc<RefCell<State>>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering the toggle card");
        let state = self.state.borrow();
        let views = self.slots.views("body");
        let colors = palette(cx);
        let drop_target = colors.drop_target;

        v_flex()
            .size_full()
            .p_2()
            .gap_2()
            .child(Label::new("Extension toggle"))
            .child(Label::new(format!("count: {}", state.count)))
            .child(Label::new(format!(
                "last URL: {}",
                state.url.as_deref().unwrap_or("none")
            )))
            .child(
                v_flex()
                    .id("toggle-card-body")
                    .flex_1()
                    .p_2()
                    .border_1()
                    .border_color(colors.border)
                    .drag_over::<DraggedNote>(move |style, _, _, _| style.border_color(drop_target))
                    .on_drop::<DraggedNote>(|dragged, _, cx| {
                        dragged
                            .0
                            .move_node(cx, "toggle-note", "toggle-card", "body", 0);
                    })
                    .children(views.is_empty().then(|| Label::new("Drop note here")))
                    .children(views),
            )
    }
}

struct Note(Extension);

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering the toggle note");
        div()
            .id("toggle-note")
            .p_2()
            .cursor_grab()
            .on_drag(DraggedNote(self.0.clone()), |dragged, _, _, cx| {
                cx.new(|_| DraggedNote(dragged.0.clone()))
            })
            .child(Label::new("Toggle note"))
    }
}

struct DraggedNote(Extension);

impl Render for DraggedNote {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Label::new("Toggle note")
    }
}

struct Aside(Extension);

impl Render for Aside {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering the toggle aside");
        let host = self.0.clone();
        v_flex()
            .size_full()
            .p_2()
            .child(
                Button::new("hide-toggle-aside", "Hide aside").on_click(move |_, cx| {
                    host.set_hidden(cx, "toggle-aside", true);
                }),
            )
    }
}
