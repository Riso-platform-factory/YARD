//! The window tour extension, loaded as `window_tour` in `cargo run dev`.
//! Without `WINDOW_TOUR`, it contributes nothing.
//! `WINDOW_TOUR=title-bar cargo run dev`: a root card, `window_tour` in `left`, and `ping` in `right`.
//! `WINDOW_TOUR=status-bar cargo run dev`: a root card with a visibility button and status bar items.
//! `WINDOW_TOUR=status-items cargo run dev`: status bar items only, for use beside the layout tour.
//! `WINDOW_TOUR=toasts cargo run dev`: a root card with information, warning, and error toast buttons.
//! `WINDOW_TOUR=modal-layer cargo run dev`: a root card with buttons opening a modal and a dimmed modal.
//! `WINDOW_TOUR=prompts cargo run dev`: a root card with buttons asking one or two questions.

use gpui::{
    App, AppContext, Bounds, Context, DismissEvent, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Pixels, Render, Size, Styled, WeakEntity,
    Window, canvas, div, rems,
};
use yard_ui::{
    Button, ButtonSize, ButtonStyle, ElevationIndex, Label, LabelSize, Modal, ModalFooter,
    ModalHeader, ModalView, Section, palette, v_flex,
};

pub fn init(cx: &mut App, ext: &yard_shell::Extension) {
    let case = std::env::var("WINDOW_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("window-tour", case).entered();
    match case.as_str() {
        "title-bar" | "status-bar" | "status-items" => {
            let status = case != "title-bar";
            let card_ext = ext.clone();
            ext.register(cx, "window_tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    status,
                    modals: false,
                    toasts: false,
                    prompts: false,
                    ext: card_ext.clone(),
                    node: node.to_string(),
                    size: Size::default(),
                })
                .into()
            });
            ext.register(cx, "window_tour.title", &[], |_, _, _, cx| {
                cx.new(|_| BarLabel).into()
            });
            ext.register(cx, "window_tour.ping", &[], |_, _, _, cx| {
                cx.new(|_| PingButton).into()
            });
            if case != "status-items" {
                ext.place_root(cx, "root", "window_tour.card");
            }
            let bar = if status { "status_bar" } else { "title_bar" };
            ext.place(cx, "title", "window_tour.title", bar, "left");
            ext.place(cx, "ping", "window_tour.ping", bar, "right");
        }
        "modal-layer" | "toasts" => {
            let modals = case == "modal-layer";
            let toasts = case == "toasts";
            let card_ext = ext.clone();
            ext.register(cx, "window_tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    status: false,
                    modals,
                    toasts,
                    prompts: false,
                    ext: card_ext.clone(),
                    node: node.to_string(),
                    size: Size::default(),
                })
                .into()
            });
            ext.place_root(cx, "root", "window_tour.card");
        }
        "prompts" => {
            let card_ext = ext.clone();
            ext.register(cx, "window_tour.card", &[], move |node, _, _, cx| {
                cx.new(|_| Card {
                    status: false,
                    modals: false,
                    toasts: false,
                    prompts: true,
                    ext: card_ext.clone(),
                    node: node.to_string(),
                    size: Size::default(),
                })
                .into()
            });
            ext.place_root(cx, "root", "window_tour.card");
        }
        _ => log::warn!(
            "unknown WINDOW_TOUR case {case:?}; known cases: title-bar, status-bar, status-items, modal-layer, toasts, prompts"
        ),
    }
}

struct BarLabel;

impl Render for BarLabel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "BAR-TITLE".into())
            .child(Label::new("window_tour").size(LabelSize::Small))
    }
}

struct PingButton;

impl Render for PingButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "BAR-PING".into()).child(
            Button::new("ping", "ping")
                .size(ButtonSize::Compact)
                .on_click(|_, _| tracing::info!("ping")),
        )
    }
}

struct TourModal {
    focus: FocusHandle,
}

impl TourModal {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl EventEmitter<DismissEvent> for TourModal {}

impl Focusable for TourModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ModalView for TourModal {}

impl Render for TourModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dismiss_header = cx.entity().downgrade();
        let dismiss_done = cx.entity().downgrade();
        Modal::new("window-tour-modal")
            .track_focus(&self.focus)
            .header(
                ModalHeader::new()
                    .headline("window_tour")
                    .on_dismiss(move |_, cx| {
                        let _ = dismiss_header.update(cx, |_, cx| {
                            tracing::info!("modal closed");
                            cx.emit(DismissEvent);
                        });
                    }),
            )
            .section(Section::new().child(Label::new("Modal opened from window_tour.")))
            .footer(
                ModalFooter::new().end_slot(
                    div().debug_selector(|| "MODAL-DONE".into()).child(
                        Button::new("done", "Done")
                            .style(ButtonStyle::Filled)
                            .layer(ElevationIndex::ModalSurface)
                            .on_click(move |_, cx| {
                                let _ = dismiss_done.update(cx, |_, cx| {
                                    tracing::info!("modal closed");
                                    cx.emit(DismissEvent);
                                });
                            }),
                    ),
                ),
            )
    }
}

struct DimmedTourModal {
    focus: FocusHandle,
}

impl DimmedTourModal {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
        }
    }
}

impl EventEmitter<DismissEvent> for DimmedTourModal {}

impl Focusable for DimmedTourModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ModalView for DimmedTourModal {
    fn dims_background(&self) -> bool {
        true
    }
}

impl Render for DimmedTourModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dismiss_header = cx.entity().downgrade();
        let dismiss_done = cx.entity().downgrade();
        Modal::new("window-tour-dimmed-modal")
            .track_focus(&self.focus)
            .header(
                ModalHeader::new()
                    .headline("window_tour")
                    .on_dismiss(move |_, cx| {
                        let _ = dismiss_header.update(cx, |_, cx| {
                            tracing::info!("modal closed");
                            cx.emit(DismissEvent);
                        });
                    }),
            )
            .section(Section::new().child(Label::new("Dimmed modal opened from window_tour.")))
            .footer(
                ModalFooter::new().end_slot(
                    div().debug_selector(|| "MODAL-DONE".into()).child(
                        Button::new("done", "Done")
                            .style(ButtonStyle::Filled)
                            .layer(ElevationIndex::ModalSurface)
                            .on_click(move |_, cx| {
                                let _ = dismiss_done.update(cx, |_, cx| {
                                    tracing::info!("modal closed");
                                    cx.emit(DismissEvent);
                                });
                            }),
                    ),
                ),
            )
    }
}

fn log_answer(answers: &'static [&'static str], answer: Option<usize>) {
    match answer.and_then(|ix| answers.get(ix).copied()) {
        Some(label) => tracing::info!(label, "answered"),
        None => tracing::info!("no answer"),
    }
}

struct Card {
    status: bool,
    modals: bool,
    toasts: bool,
    prompts: bool,
    ext: yard_shell::Extension,
    node: String,
    size: Size<Pixels>,
}

impl Card {
    fn prepaint(
        entity: WeakEntity<Self>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let changed = entity
            .update(cx, |card, _| {
                if card.size == bounds.size {
                    return false;
                }
                tracing::trace!(
                    node = %card.node,
                    width = %bounds.size.width,
                    height = %bounds.size.height,
                    "card size changed"
                );
                card.size = bounds.size;
                true
            })
            .unwrap_or(false);
        if changed {
            // Prepaint invalidations do not schedule a redraw in GPUI.
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            });
        }
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = format!("{} × {}", self.size.width, self.size.height);
        tracing::trace!(node = %self.node, size = %size, "rendering card");
        let palette = palette(cx);
        let entity = cx.entity().downgrade();
        let focused = yard_shell::node_focus_handle(cx, &self.node)
            .is_some_and(|handle| handle.is_focused(window));
        let mut card = v_flex();
        if self.status {
            let hidden = yard_shell::is_hidden(cx, "title") || yard_shell::is_hidden(cx, "ping");
            let ext = self.ext.clone();
            card = card.child(
                div()
                    .debug_selector(|| "STATUS-VISIBILITY".into())
                    .self_start()
                    .child(
                        Button::new(
                            "status-visibility",
                            if hidden {
                                "show status items"
                            } else {
                                "hide status items"
                            },
                        )
                        .on_click(move |_, cx| {
                            let hidden = yard_shell::is_hidden(cx, "title")
                                || yard_shell::is_hidden(cx, "ping");
                            ext.set_hidden(cx, "title", !hidden);
                            ext.set_hidden(cx, "ping", !hidden);
                        }),
                    ),
            );
        }
        if self.toasts {
            for (label, kind, text) in [
                ("info", yard_ui::ToastKind::Info, "An information message."),
                ("warning", yard_ui::ToastKind::Warning, "A warning message."),
                ("error", yard_ui::ToastKind::Error, "An error message."),
            ] {
                let ext = self.ext.clone();
                card = card.child(
                    div()
                        .debug_selector(move || format!("SHOW-TOAST-{label}"))
                        .self_start()
                        .child(Button::new(label, label).on_click(move |window, cx| {
                            let mut toast = yard_ui::Toast::new(kind, text);
                            if kind == yard_ui::ToastKind::Error {
                                toast =
                                    toast.action("Retry", |_, _| tracing::info!("retry clicked"));
                            }
                            ext.show_toast(toast, window, cx);
                        })),
                );
            }
        }
        if self.modals {
            let open_ext = self.ext.clone();
            let dimmed_ext = self.ext.clone();
            card = card
                .child(
                    div()
                        .debug_selector(|| "OPEN-MODAL".into())
                        .self_start()
                        .child(Button::new("open-modal", "open modal").on_click(
                            move |window, cx| {
                                open_ext.toggle_modal(window, cx, |_, cx| TourModal::new(cx));
                            },
                        )),
                )
                .child(
                    div()
                        .debug_selector(|| "OPEN-DIMMED-MODAL".into())
                        .self_start()
                        .child(
                            Button::new("open-dimmed-modal", "open dimmed modal").on_click(
                                move |window, cx| {
                                    dimmed_ext
                                        .toggle_modal(window, cx, |_, cx| DimmedTourModal::new(cx));
                                },
                            ),
                        ),
                );
        }
        if self.prompts {
            let ask_ext = self.ext.clone();
            let twice_ext = self.ext.clone();
            card = card
                .child(
                    div()
                        .debug_selector(|| "ASK-PROMPT".into())
                        .self_start()
                        .child(Button::new("ask", "ask").on_click(move |window, cx| {
                            const ANSWERS: &[&str] = &["Save", "Don't Save", "Cancel"];
                            ask_ext.prompt(
                                yard_shell::Prompt::new(
                                    gpui::PromptLevel::Warning,
                                    "Save changes?",
                                    ANSWERS,
                                )
                                .detail("Your changes will be lost if you don't save them."),
                                window,
                                cx,
                                |answer, _| log_answer(ANSWERS, answer),
                            );
                        })),
                )
                .child(
                    div()
                        .debug_selector(|| "ASK-TWICE".into())
                        .self_start()
                        .child(Button::new("ask-twice", "ask twice").on_click(
                            move |window, cx| {
                                const FIRST: &[&str] = &["OK"];
                                const SECOND: &[&str] = &["Yes", "No"];
                                twice_ext.prompt(
                                    yard_shell::Prompt::new(
                                        gpui::PromptLevel::Info,
                                        "First question",
                                        FIRST,
                                    ),
                                    window,
                                    cx,
                                    |answer, _| log_answer(FIRST, answer),
                                );
                                twice_ext.prompt(
                                    yard_shell::Prompt::new(
                                        gpui::PromptLevel::Info,
                                        "Second question",
                                        SECOND,
                                    ),
                                    window,
                                    cx,
                                    |answer, _| log_answer(SECOND, answer),
                                );
                            },
                        )),
                );
        }
        card.debug_selector(|| format!("CARD-{}", self.node))
            .relative()
            .size_full()
            .p(rems(0.5))
            .gap(rems(0.25))
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(Label::new(self.node.clone()))
            .child(Label::new(size))
            .children(focused.then(|| Label::new("focused")))
            .child(
                canvas(
                    move |bounds, window, cx| Self::prepaint(entity, bounds, window, cx),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}
