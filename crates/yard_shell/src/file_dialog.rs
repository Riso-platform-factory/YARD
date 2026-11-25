//! Native file dialogs available through the extension handle.

use futures::channel::oneshot;
use gpui::{App, PathPromptOptions};
use std::path::{Path, PathBuf};
use tracing::{Instrument, Span};

impl crate::Extension {
    /// Opens the system chooser. The callback receives the selected paths, or
    /// `None` if the request is refused or the dialog ends without a selection.
    // TODO(file-dialogs-filters): filters and a starting folder for choose_paths. Unblocked when GPUI's PathPromptOptions has them.
    pub fn choose_paths(
        &self,
        options: PathPromptOptions,
        cx: &mut App,
        on_paths: impl FnOnce(Option<Vec<PathBuf>>, &mut App) + 'static,
    ) {
        let span = tracing::info_span!(
            "file-dialog",
            extension = self.name.as_str(),
            kind = "open",
            files = options.files,
            directories = options.directories,
            multiple = options.multiple,
        );
        let group = span.enter();
        if !options.files && !options.directories {
            tracing::info!("refused: neither files nor directories requested");
            drop(group);
            cx.defer(move |cx| span.in_scope(|| on_paths(None, cx)));
            return;
        }
        if options.files && options.directories && !cx.can_select_mixed_files_and_dirs() {
            tracing::warn!(
                "the system cannot select files and folders together; showing a folder chooser"
            );
        }
        let receiver = cx.prompt_for_paths(options);
        tracing::debug!("shown");
        drop(group);
        finish_dialog(receiver, span, Vec::as_slice, cx, on_paths);
    }

    /// Opens the system save chooser in `directory` with the suggested name.
    /// The callback receives the chosen path, or `None` without a selection.
    pub fn choose_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
        cx: &mut App,
        on_path: impl FnOnce(Option<PathBuf>, &mut App) + 'static,
    ) {
        let span =
            tracing::info_span!("file-dialog", extension = self.name.as_str(), kind = "save");
        let receiver = span.in_scope(|| {
            let receiver = cx.prompt_for_new_path(directory, suggested_name);
            tracing::debug!("shown");
            receiver
        });
        finish_dialog(receiver, span, std::slice::from_ref, cx, on_path);
    }
}

fn finish_dialog<T: 'static>(
    receiver: oneshot::Receiver<gpui::Result<Option<T>>>,
    span: Span,
    as_paths: fn(&T) -> &[PathBuf],
    cx: &mut App,
    on_selection: impl FnOnce(Option<T>, &mut App) + 'static,
) {
    cx.spawn(async move |cx| {
        async move {
            let selection = match receiver.await {
                Ok(Ok(Some(selection))) => {
                    let paths = as_paths(&selection);
                    tracing::debug!(count = paths.len() as u64, ?paths, "chosen");
                    Some(selection)
                }
                Ok(Ok(None)) => {
                    tracing::info!("cancelled");
                    None
                }
                Ok(Err(error)) => {
                    log::error!("could not show file dialog: {error}");
                    None
                }
                Err(_) => {
                    tracing::info!("no answer");
                    None
                }
            };
            cx.update(|cx| on_selection(selection, cx));
        }
        .instrument(span)
        .await
    })
    .detach();
}

#[cfg(test)]
mod tests;
