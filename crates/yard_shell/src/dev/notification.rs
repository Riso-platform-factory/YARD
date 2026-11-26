//! System notifications available through the extension handle.

use gpui::{App, Global, SystemNotification, SystemNotificationResponse};
use std::collections::{BTreeMap, btree_map::Entry};
use tracing::Instrument;

type Handler = Box<dyn FnMut(SystemNotificationResponse, &mut App)>;

/// The response handler of each extension, `None` while it runs.
#[derive(Default)]
struct Notifications {
    installed: bool,
    handlers: BTreeMap<String, Option<Handler>>,
}

impl Global for Notifications {}

impl crate::Extension {
    /// Posts a notification, or replaces this extension's one with the same tag.
    pub fn show_notification(&self, notification: SystemNotification, cx: &mut App) {
        let _group = tracing::info_span!(
            "notification",
            extension = self.name.as_str(),
            tag = notification.tag.as_ref(),
            kind = "show",
        )
        .entered();
        install(cx);
        let title = notification.title.clone();
        let actions = notification.actions.len();
        cx.show_system_notification(SystemNotification {
            tag: format!("{}/{}", self.name, notification.tag).into(),
            ..notification
        });
        tracing::debug!(title = title.as_ref(), actions, "posted");
    }

    /// Takes back this extension's notification with this tag.
    pub fn dismiss_notification(&self, tag: &str, cx: &mut App) {
        let _group = tracing::info_span!(
            "notification",
            extension = self.name.as_str(),
            tag,
            kind = "dismiss",
        )
        .entered();
        cx.dismiss_system_notification(&format!("{}/{tag}", self.name));
        tracing::debug!("dismissed");
    }

    /// Registers the handler for the user's clicks on this extension's notifications.
    pub fn on_notification_response(
        &self,
        cx: &mut App,
        handler: impl FnMut(SystemNotificationResponse, &mut App) + 'static,
    ) {
        let _group = tracing::info_span!(
            "notification-handler-register",
            extension = self.name.as_str()
        )
        .entered();
        install(cx);
        match cx
            .default_global::<Notifications>()
            .handlers
            .entry(self.name.clone())
        {
            Entry::Vacant(entry) => {
                entry.insert(Some(Box::new(handler)));
                tracing::debug!("registered");
            }
            Entry::Occupied(entry) => {
                log::error!(
                    "extension {} already has a notification response handler",
                    entry.key()
                );
            }
        }
    }
}

/// Installs the shell's one GPUI response handler, at the first use, since on
/// macOS installing it touches the notification centre.
fn install(cx: &mut App) {
    if std::mem::replace(&mut cx.default_global::<Notifications>().installed, true) {
        return;
    }
    cx.on_system_notification_response(respond);
}

fn respond(response: SystemNotificationResponse, cx: &mut App) {
    let (extension, tag) = response
        .tag
        .split_once('/')
        .map(|(extension, tag)| (extension.to_string(), tag.to_string()))
        .unwrap_or_else(|| (String::new(), response.tag.to_string()));
    let action = response.action_id.as_deref().unwrap_or("");
    let span = tracing::info_span!(
        "notification-response",
        extension = extension.as_str(),
        tag = tag.as_str(),
        action,
    );
    // GPUI calls this outside an update, so the handler runs in one that
    // flushes its effects.
    cx.spawn(async move |cx| {
        async move {
            cx.update(|cx| {
                // Taken out of the global, so the handler may post, dismiss, or register.
                let Some(mut handler) = cx
                    .default_global::<Notifications>()
                    .handlers
                    .get_mut(&extension)
                    .and_then(Option::take)
                else {
                    tracing::info!("no extension handles this notification");
                    return;
                };
                handler(
                    SystemNotificationResponse {
                        tag: tag.into(),
                        action_id: response.action_id,
                    },
                    cx,
                );
                tracing::debug!("handled");
                if let Some(slot) = cx
                    .default_global::<Notifications>()
                    .handlers
                    .get_mut(&extension)
                {
                    *slot = Some(handler);
                }
            })
        }
        .instrument(span)
        .await
    })
    .detach();
}
