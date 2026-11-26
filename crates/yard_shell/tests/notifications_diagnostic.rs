// Every test in this binary takes `LOCK`, runs under a scoped `tracing`
// subscriber (matching the application, where `yard_log` always installs one
// so `tracing`'s `log` feature never forwards `tracing` spans/events into
// `log`), and shares the one process-wide diagnostic logger so tests never
// collide on `log::set_logger` or see each other's records.

use gpui::{
    App, SystemNotification, SystemNotificationAction, SystemNotificationResponse, TestAppContext,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Mutex, Once},
};

static INIT_LOGGER: Once = Once::new();
static LOCK: Mutex<()> = Mutex::new(());
static DIAGNOSTICS: Mutex<Vec<(String, log::Level, String)>> = Mutex::new(Vec::new());

struct Diagnostics;

impl log::Log for Diagnostics {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        DIAGNOSTICS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((
                record.target().to_string(),
                record.level(),
                record.args().to_string(),
            ));
    }

    fn flush(&self) {}
}

fn with_diagnostics(cx: &mut TestAppContext, test: impl FnOnce(&mut TestAppContext)) {
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        INIT_LOGGER.call_once(|| {
            log::set_logger(&Diagnostics).unwrap();
            log::set_max_level(log::LevelFilter::Trace);
        });
        DIAGNOSTICS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        test(cx);
    });
}

fn records() -> Vec<(String, log::Level, String)> {
    DIAGNOSTICS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn errors() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(target, level, _)| {
            target.starts_with("yard_shell") && *level == log::Level::Error
        })
        .map(|(_, _, message)| message)
        .collect()
}

fn warnings() -> Vec<String> {
    records()
        .into_iter()
        .filter(|(target, level, _)| target.starts_with("yard_shell") && *level == log::Level::Warn)
        .map(|(_, _, message)| message)
        .collect()
}

#[gpui::test]
fn second_handler_registration_logs_diagnostic_error_and_other_operations_do_not(
    cx: &mut TestAppContext,
) {
    with_diagnostics(cx, |cx| {
        cx.update(|cx| cx.set_app_identity("dev.yard.test", "Yard Test"));
        let alpha = cx.update(|cx| yard_shell::Extension::new(cx, "alpha").unwrap());
        let _quiet = cx.update(|cx| yard_shell::Extension::new(cx, "quiet").unwrap());

        let received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
            Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let sink = received.clone();
            alpha.on_notification_response(
                cx,
                move |response: SystemNotificationResponse, _: &mut App| {
                    sink.borrow_mut().push(response);
                },
            );
            alpha.show_notification(
                SystemNotification {
                    tag: "build".into(),
                    title: "Build finished".into(),
                    body: "3 warnings".into(),
                    actions: Vec::new(),
                },
                cx,
            );
            alpha.dismiss_notification("build", cx);
        });

        for response in [
            SystemNotificationResponse {
                tag: "alpha/build".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "quiet/ping".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "ghost/x".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "orphan".into(),
                action_id: None,
            },
        ] {
            cx.simulate_system_notification_response(response);
            cx.run_until_parked();
        }

        assert!(
            errors().is_empty(),
            "normal notification operations must not log diagnostic errors: {:?}",
            errors()
        );
        assert!(
            warnings().is_empty(),
            "normal notification operations must not log diagnostic warnings: {:?}",
            warnings()
        );

        cx.update(|cx| {
            alpha.on_notification_response(cx, |_: SystemNotificationResponse, _: &mut App| {});
        });

        assert_eq!(
            errors().len(),
            1,
            "a second handler registration must log one diagnostic error: {:?}",
            errors()
        );
        assert!(
            warnings().is_empty(),
            "a second handler registration must not log diagnostic warnings: {:?}",
            warnings()
        );

        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/build".into(),
            action_id: Some("open".into()),
        });
        cx.run_until_parked();
        assert_eq!(
            received.borrow().len(),
            2,
            "the first handler must keep answering after the refused registration"
        );
        assert_eq!(errors().len(), 1, "{:?}", errors());
    });
}

#[gpui::test]
fn diagnostic_log_never_records_notification_title_or_body(cx: &mut TestAppContext) {
    with_diagnostics(cx, |cx| {
        let secret_title = "PRIVATE-NOTIFICATION-TITLE-9f3a";
        let secret_body = "PRIVATE-NOTIFICATION-BODY-7c2e";

        cx.update(|cx| cx.set_app_identity("dev.yard.test", "Yard Test"));
        let alpha = cx.update(|cx| yard_shell::Extension::new(cx, "alpha").unwrap());

        let received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
            Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            let sink = received.clone();
            alpha.on_notification_response(
                cx,
                move |response: SystemNotificationResponse, _: &mut App| {
                    sink.borrow_mut().push(response);
                },
            );
            alpha.show_notification(
                SystemNotification {
                    tag: "build".into(),
                    title: secret_title.into(),
                    body: secret_body.into(),
                    actions: vec![SystemNotificationAction {
                        id: "open".into(),
                        label: "Open".into(),
                    }],
                },
                cx,
            );
        });

        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/build".into(),
            action_id: Some("open".into()),
        });
        cx.run_until_parked();

        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: "alpha/build".into(),
            action_id: None,
        });
        cx.run_until_parked();

        cx.update(|cx| {
            alpha.dismiss_notification("build", cx);
        });

        assert_eq!(
            received.borrow().len(),
            2,
            "both simulated responses must have reached the handler"
        );

        let all_records = records();
        for needle in [secret_title, secret_body] {
            assert!(
                all_records
                    .iter()
                    .all(|(_, _, message)| !message.contains(needle)),
                "diagnostic log must never contain notification title or body ({needle}): {all_records:#?}"
            );
        }
    });
}
