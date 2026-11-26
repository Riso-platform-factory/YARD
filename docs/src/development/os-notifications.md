# os-notifications: notifications in the system's notification centre through the extension handle

An extension that finishes long work, receives a message, or needs the user's attention while the user is in another application posts a notification to the system's notification centre: `UNUserNotificationCenter` on macOS, the freedesktop notification service on Linux, toast notifications on Windows. GPUI posts them already (`App::show_system_notification`, `dismiss_system_notification`, `on_system_notification_response`), and YARD's startup already gives GPUI the product's identity (`cx.set_app_identity(identifier, name)` in `yard_app`), which Windows needs. This slice gives notifications to extensions through the extension handle, keeps each extension's notifications apart from the others', brings the user's click back to the extension that posted, and has the lifecycle group and diagnostic lines every engine has.

## What an extension calls

```rust
// Post, or replace the notification with the same tag.
ext.show_notification(
    gpui::SystemNotification {
        tag: "build".into(),
        title: "Build finished".into(),
        body: "3 warnings".into(),
        actions: vec![gpui::SystemNotificationAction { id: "open".into(), label: "Open".into() }],
    },
    cx,
);

// Take it back.
ext.dismiss_notification("build", cx);

// The user clicked it, or one of its buttons.
ext.on_notification_response(cx, |response: gpui::SystemNotificationResponse, cx| {
    // response.tag == "build"; response.action_id == Some("open") or None for the body.
});
```

- The three are methods of `yard_shell::Extension`, beside `show_toast`. The types are GPUI's own `SystemNotification`, `SystemNotificationAction`, and `SystemNotificationResponse`; YARD adds no type of its own.
- Notifications belong to the application, not to a window, as GPUI's do: the calls take `&mut App`.
- **Tags are the extension's own.** The handle posts the tag as `<extension>/<tag>`, so two extensions with the same tag never replace or dismiss each other's notifications, and gives the response back with the extension's own tag.
- **The response goes to the extension that posted.** The shell holds GPUI's single response handler and gives each response to the handler of the extension named in the tag, on the main thread. One handler per extension: a second registration is refused and the first stays, as `on_open_url` does.
- **Posting is the extension's decision.** GPUI shows the notification even when the application is in front (on macOS as a banner). An extension that wants a notification only when the user is elsewhere asks first, for example `cx.active_window().is_none()`, and shows a toast otherwise.
- What each system does with a notification, as GPUI does it at the pinned revision:
  - macOS: shown only when the application runs from an app bundle; under `cargo run` GPUI turns notifications off and logs it. The first notification asks the user's permission; when it is denied, nothing is shown. Same tag replaces; dismiss removes it.
  - Linux: shown through the session's notification service with the product's name; buttons as the service allows. Same tag does not replace, and dismiss does nothing: the notification ages out on its own.
  - Windows: a toast under the product's AppUserModelID. Same tag replaces; dismiss removes it.

## Logging

- A lifecycle group `notification{extension, tag, kind}` (`kind` is `show` or `dismiss`) surrounds the call to GPUI. `debug`: `posted` with the title and the number of actions, or `dismissed` (the lifecycle trace is never stored).
- A lifecycle group `notification-response{extension, tag, action}` (`action` is the button's id, or empty for the body) surrounds the extension's handler while it runs. `debug`: `handled`.
- A lifecycle group `notification-handler-register{extension}` surrounds the registration. `debug`: `registered`.
- `info`: a response for an extension that registered no handler, or whose tag names no extension (for example a notification left from an earlier run of another build).
- `error`, also in the diagnostic log: a second handler for the same extension.
- GPUI does not report a notification it could not show; its own lines (not a bundle, permission denied, no notification service) are in the diagnostic log.

## Parts

One part, `yard_shell/notification`, in `crates/yard_shell/src/dev/notification.rs` (the crate gains `src/dev.rs` again). It adds the three methods to `Extension` and keeps its handlers in its own global, without changing the production part `yard_shell/shell`.

- **Acceptance tests** (in `crates/yard_shell/tests/`, `#![cfg(feature = "dev")]`), with GPUI's test platform (`shown_system_notifications`, `delivered_system_notifications`, `dismissed_system_notifications`, `simulate_system_notification_response`; the test sets an app identity first, as the startup does): the notification reaches GPUI with the namespaced tag and its title, body, and actions unchanged; the same tag from two extensions gives two notifications; dismiss reaches GPUI with the namespaced tag; a response reaches only the handler of the extension that posted, with its own tag and the action's id; a response with no handler, or for no extension, runs nothing and does not panic; a second handler is refused and the first still answers; a handler can post or dismiss from inside itself; and, in their own files, the groups, their fields, and the levels above.
- **Test extension** `examples/notification_tour`, named in `review.rs` while the part is under review. Without `NOTIFICATION_TOUR` it does nothing. With `NOTIFICATION_TOUR=simple`, `actions`, `replace`, `dismiss`, or `away`, a root card has a button for that case and shows the last response (`body`, or the button's id):
  - `simple`: posts a title and a body.
  - `actions`: posts with two buttons, `Reply` and `Mark read`.
  - `replace`: each press posts the same tag with a count, so one notification stays.
  - `dismiss`: one button posts, another dismisses.
  - `away`: posts five seconds after the press only if no window of the application is in front, and otherwise says on the card that it did not.
- **Run by the lead on macOS** from a review bundle (`cargo xtask bundle macos --dev --config <a product.toml other than the sample's> --env NOTIFICATION_TOUR=<case>`), since `cargo run dev` is not a bundle, with every case; the first post asks the permission. `cargo run dev` is run too, to see GPUI's line that notifications are off. Linux and Windows build in the runner checks; their notifications need a person, so they are run by hand on those systems when the user has one, and the report says which were run.

## Not in this slice

- Responses after the application was quit: what the system does when the user clicks a notification of an application that no longer runs (macOS may start it and deliver the response; Windows and Linux do not) is not checked.
- Sounds, images, badges, scheduled notifications, and asking for permission before the first post: GPUI at the pinned revision has none of them.
- A notification centre or history inside the application: in-app messages are toasts.
