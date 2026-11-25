use gpui::{
    App, AppContext, Context, IntoElement, ParentElement, PathPromptOptions, Render, Styled, Window,
};
use std::path::PathBuf;
use yard_shell::Extension;
use yard_ui::{Button, Label, palette, v_flex};

pub fn init(cx: &mut App, ext: &Extension) {
    let case = std::env::var("FILE_DIALOG_TOUR").unwrap_or_default();
    if case.is_empty() {
        return;
    }
    let _group = tracing::info_span!("file-dialog-tour", case).entered();
    let (label, options) = match case.as_str() {
        "open" => ("Open file", Some((true, false, false))),
        "open-many" => ("Open files", Some((true, false, true))),
        "folders" => ("Open folders", Some((false, true, true))),
        "mixed" => ("Open files and folders", Some((true, true, true))),
        "save" => ("Choose save path", None),
        _ => {
            log::warn!(
                "unknown FILE_DIALOG_TOUR case {case:?}; known cases: open, open-many, folders, mixed, save"
            );
            return;
        }
    };
    let ext_for_card = ext.clone();
    ext.register(cx, "file_dialog_tour.card", &[], move |_, _, _, cx| {
        cx.new(|_| Card {
            ext: ext_for_card.clone(),
            label,
            options,
            result: Vec::new(),
        })
        .into()
    });
    ext.place_root(cx, "root", "file_dialog_tour.card");
}

struct Card {
    ext: Extension,
    label: &'static str,
    options: Option<(bool, bool, bool)>,
    result: Vec<String>,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering file dialog card");
        let card = cx.weak_entity();
        let ext = self.ext.clone();
        let options = self.options;
        let palette = palette(cx);
        v_flex()
            .size_full()
            .p_2()
            .gap_1()
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .child(
                Button::new("choose-path", self.label).on_click(move |_, cx| {
                    let card = card.clone();
                    let show_result = move |paths: Option<Vec<PathBuf>>, cx: &mut App| {
                        let _ = card.update(cx, |card, cx| {
                            card.result = match paths {
                                Some(paths) if paths.is_empty() => vec!["(no paths)".into()],
                                Some(paths) => paths
                                    .into_iter()
                                    .map(|path| path.display().to_string())
                                    .collect(),
                                None => vec!["cancelled".into()],
                            };
                            cx.notify();
                        });
                    };
                    if let Some((files, directories, multiple)) = options {
                        ext.choose_paths(
                            PathPromptOptions {
                                files,
                                directories,
                                multiple,
                                prompt: None,
                            },
                            cx,
                            show_result,
                        );
                    } else if let Some(home) = dirs::home_dir() {
                        ext.choose_new_path(&home, Some("Untitled.txt"), cx, move |path, cx| {
                            show_result(path.map(|path| vec![path]), cx);
                        });
                    } else {
                        log::error!("could not find the home directory for the save tour");
                        show_result(None, cx);
                    }
                }),
            )
            .children(self.result.iter().cloned().map(Label::new))
    }
}
