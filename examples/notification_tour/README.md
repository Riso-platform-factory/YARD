# Notification tour

Without `NOTIFICATION_TOUR`, the extension contributes nothing.

With a case, a root card shows that case's buttons and, below them, `last response: none` until a notification is clicked. A click on the body shows `last response: body, tag <tag>`; a click on a button shows its id, for example `last response: reply, tag message`.

## Running

- **Linux:** `NOTIFICATION_TOUR=<case> cargo run dev`.
- **Windows** (PowerShell): `$env:NOTIFICATION_TOUR = "<case>"; cargo run dev`.
- **macOS.** Notifications need an app bundle. Under `cargo run dev`, GPUI logs that notifications are off. Keep the bundle in the default `dist/` directory. Avoid building it in or moving it to `/tmp` or any other temporary directory. macOS 26 refused notifications from `/private/tmp` even after LaunchServices registration.

  Create `~/yard-check/product.toml` with `name = "yard-check"`, `version = "0.1.0"`, `identifier = "vn.risotech.yard-check"`, `url_scheme = "yard-check"`, and `icon = "icon.png"`. Copy `app/yard/icon.png` to `~/yard-check/icon.png` because the icon path is relative to the config. Build and open the bundle.

  ```sh
  cargo xtask bundle macos --dev --config ~/yard-check/product.toml --env "NOTIFICATION_TOUR=<case>"
  open dist/yard-check.app
  ```

  The first post asks for permission, and its notification is sent while the request is still open, so it is not shown: after allowing, press the button again. The lifecycle trace is in `~/Library/Logs/yard-check/lifecycle.jsonl`.

## Cases

| Case | Variable | What to look for |
| --- | --- | --- |
| `simple` | `NOTIFICATION_TOUR=simple` | Click **Post**. A notification titled `Hello` with the body `From the notification tour`. Click it: the card shows `last response: body, tag hello`. |
| `actions` | `NOTIFICATION_TOUR=actions` | Click **Post with buttons**. The notification `New message` has the buttons **Reply** and **Mark read**. Each button shows `reply` or `read` with `tag message`; the body shows `body`. |
| `replace` | `NOTIFICATION_TOUR=replace` | Click **Post count** several times. On macOS and Windows one notification stays, its body `Count <n>` with the latest count. On Linux each press shows a new one. |
| `dismiss` | `NOTIFICATION_TOUR=dismiss` | Click **Post**, then **Dismiss**. On macOS and Windows the notification `Dismissable` goes away. On Linux it stays until it ages out. |
| `away` | `NOTIFICATION_TOUR=away` | Click **Post in five seconds**: the card shows `waiting five seconds`. Switch to another application, so no window of the tour is active: after five seconds, the notification `While you were away` appears and the card shows `posted`. Stay in the window instead: no notification, and the card shows `not posted: a window is in front`. |

An unknown value, such as `NOTIFICATION_TOUR=nope`, warns with the known cases and contributes nothing.

## Lifecycle lines

- At startup: `notification-tour{case=<case>}`, and inside it `notification-handler-register{extension=notification_tour}` with debug `registered`.
- Each post: `notification{extension=notification_tour, tag=<tag>, kind=show}` with debug `posted` and the `title` and `actions` count. The tag is the tour's own (`hello`, `message`, `counter`, `dismissable`, `away`); GPUI receives `notification_tour/<tag>`.
- Each dismiss: `notification{extension=notification_tour, tag=dismissable, kind=dismiss}` with debug `dismissed`.
- Each click: `notification-response{extension=notification_tour, tag=<tag>, action=<id or empty>}` with debug `handled`.
- `away`: `notification-tour-away` with info `posted: no window is in front` (and the `notification` group inside it) or info `not posted: a window is in front`.

No title or body goes to the diagnostic log.
