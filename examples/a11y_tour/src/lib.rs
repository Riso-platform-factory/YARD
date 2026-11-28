use gpui::{
    App, AppContext, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, ParentElement, Render, Role, Styled, Window,
};
use std::path::{Path, PathBuf};
use yard_product::Product;
use yard_shell::Extension;
use yard_ui::{
    Button, IconButton, IconName, Label, List, ListItem, Modal, ModalFooter, ModalHeader,
    ModalView, Section, Tab, TabBar, TextInput, Toast, ToastKind, h_flex, v_flex,
};

pub fn init(cx: &mut App, extension: &Extension, product: Product) {
    if std::env::var("A11Y_TOUR").as_deref() != Ok("1") {
        return;
    }
    let _group = tracing::info_span!("a11y-tour-init").entered();
    let dump_path = yard_paths::Paths::from_env(&product)
        .logs_dir()
        .join("a11y-tree.json");
    let extension_for_card = extension.clone();
    extension.register(cx, "a11y_tour.card", &[], move |_, _, window, cx| {
        cx.new(|cx| Card {
            extension: extension_for_card.clone(),
            dump_path: dump_path.clone(),
            input: cx.new(|cx| {
                TextInput::new(window, cx)
                    .label("Name")
                    .placeholder("Your name")
            }),
            tab: 0,
            details_open: true,
            row: 0,
        })
        .into()
    });
    extension.place_root(cx, "a11y-tour-card", "a11y_tour.card");
    tracing::debug!("placed the accessibility tour card");
    extension.register(cx, "a11y_tour.status", &[], |_, _, _, cx| {
        cx.new(|_| TourStatus).into()
    });
    extension.place(
        cx,
        "a11y-tour-status",
        "a11y_tour.status",
        "status_bar",
        "left",
    );
    tracing::debug!("placed the accessibility tour status");
}

struct TourStatus;

impl Render for TourStatus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Label::new("Accessibility tour")
    }
}

struct Card {
    extension: Extension,
    dump_path: PathBuf,
    input: Entity<TextInput>,
    tab: usize,
    details_open: bool,
    row: usize,
}

const ROWS: [&str; 3] = ["First choice", "Second choice", "Third choice"];

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = yard_ui::palette(cx);
        let tabs = ["Overview", "Details"]
            .into_iter()
            .enumerate()
            .take(if self.details_open { 2 } else { 1 })
            .map(|(index, label)| {
                let view = cx.weak_entity();
                let close = view.clone();
                let tab = Tab::new(("tour-tab", index), label)
                    .selected(index == self.tab)
                    .on_click(move |_, cx| {
                        let _ = view.update(cx, |card, cx| {
                            card.tab = index;
                            cx.notify();
                        });
                    });
                if index == 1 {
                    tab.on_close(move |_, cx| {
                        let _ = close.update(cx, |card, cx| {
                            card.tab = 0;
                            card.details_open = false;
                            cx.notify();
                        });
                    })
                } else {
                    tab
                }
            })
            .collect();

        let rows = ROWS.into_iter().enumerate().map(|(index, label)| {
            let view = cx.weak_entity();
            ListItem::new(("tour-choice", index))
                .aria_role(Role::ListBoxOption)
                .aria_label(label)
                .aria_selected(index == self.row)
                .on_click(move |_, cx| {
                    let _ = view.update(cx, |card, cx| {
                        card.row = index;
                        cx.notify();
                    });
                })
                .child(Label::new(label))
        });

        let view = cx.weak_entity();
        let select_next =
            Button::new("next-choice", "Select next choice").on_click(move |_, cx| {
                let _ = view.update(cx, |card, cx| {
                    card.row = (card.row + 1) % ROWS.len();
                    cx.notify();
                });
            });
        let view = cx.weak_entity();
        let refresh = IconButton::new("refresh", IconName::GenericRestore)
            .aria_label("Refresh")
            .on_click(move |_, cx| {
                let _ = view.update(cx, |card, cx| {
                    card.tab = 0;
                    card.details_open = true;
                    card.row = 0;
                    cx.notify();
                });
            });

        let extension = self.extension.clone();
        let dump_path = self.dump_path.clone();
        let about = Button::new("open-about", "Open About").on_click(move |window, cx| {
            extension.toggle_modal(window, cx, |_, cx| About {
                focus: cx.focus_handle(),
                dump_path: dump_path.clone(),
            });
        });
        let toast_buttons = [
            (
                "info-toast",
                "Show info toast",
                ToastKind::Info,
                "Information from the tour",
            ),
            (
                "error-toast",
                "Show error toast",
                ToastKind::Error,
                "Error from the tour",
            ),
        ]
        .map(|(id, label, kind, text)| {
            let extension = self.extension.clone();
            Button::new(id, label).on_click(move |window, cx| {
                extension.show_toast(Toast::new(kind, text), window, cx);
            })
        });

        v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(colors.border)
            .child(h_flex().gap_2().child(select_next).child(refresh))
            .child(self.input.clone())
            .child(TabBar::new("tour-tabs").tabs(tabs))
            .child(List::new().id("tour-choices").children(rows))
            .child(
                h_flex()
                    .gap_2()
                    .child(about)
                    .children(toast_buttons)
                    .child(dump_button(self.dump_path.clone())),
            )
    }
}

struct About {
    focus: FocusHandle,
    dump_path: PathBuf,
}

impl EventEmitter<DismissEvent> for About {}
impl ModalView for About {}

impl Focusable for About {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for About {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.weak_entity();
        Modal::new("a11y-tour-about")
            .track_focus(&self.focus)
            .header(
                ModalHeader::new()
                    .headline("About")
                    .on_dismiss(move |_, cx| {
                        let _ = view.update(cx, |_, cx| cx.emit(DismissEvent));
                    }),
            )
            .section(Section::new().child(Label::new("Accessibility tour")))
            .footer(ModalFooter::new().end_slot(dump_button(self.dump_path.clone())))
    }
}

fn dump_button(path: PathBuf) -> Button {
    Button::new("dump-tree", "Dump tree").on_click(move |window, _| dump_tree(window, &path))
}

fn dump_tree(window: &Window, path: &Path) {
    let _group = tracing::info_span!("a11y-tour-dump").entered();
    let tree = window
        .is_a11y_active()
        .then(|| window.debug_a11y_tree_json())
        .flatten();
    let Some(tree) = tree else {
        tracing::info!(reason = "no assistive technology is active", "refused");
        return;
    };
    match yard_paths::write_atomic(path, tree.as_bytes()) {
        Ok(()) => tracing::debug!(path = %path.display(), "wrote tree"),
        Err(error) => tracing::error!(path = %path.display(), %error, "could not write tree"),
    }
}
