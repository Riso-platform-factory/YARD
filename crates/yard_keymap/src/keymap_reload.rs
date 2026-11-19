use crate::{load_user_keymap, user_keymap::DefaultBindings};
use gpui::App;
use std::{path::PathBuf, time::Duration};
use tracing::Instrument;
use yard_paths::FileCheck;

/// Checks `path` once a second and binds the user's keys again when it changes.
pub fn watch_user_keymap(cx: &mut App, path: PathBuf) {
    let span = tracing::info_span!("keymap-watch", path = %path.display());
    let _group = span.enter();
    let defaults = cx.global::<DefaultBindings>().0.clone();
    let mut check = FileCheck::new(path.clone());
    check.check();
    tracing::debug!("started");
    let task_span = span.clone();
    cx.spawn(async move |cx| {
        async move {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if check.check().is_some() {
                    cx.update(|cx| {
                        let _group =
                            tracing::info_span!("keymap-reload", path = %path.display()).entered();
                        cx.clear_key_bindings();
                        cx.bind_keys(defaults.clone());
                        tracing::debug!(count = defaults.len(), "defaults bound");
                        load_user_keymap(cx, &path);
                    });
                }
            }
        }
        .instrument(task_span)
        .await
    })
    .detach();
}
