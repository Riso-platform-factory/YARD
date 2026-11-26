use gpui::{
    App, SystemNotification, SystemNotificationAction, SystemNotificationResponse, TestAppContext,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use yard_shell::Extension;

fn set_identity(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_app_identity("dev.yard.test", "Yard Test"));
}

fn extension(cx: &mut TestAppContext, name: &str) -> Extension {
    cx.update(|cx| Extension::new(cx, name).unwrap())
}

fn sample_notification(tag: &str, title: &str, body: &str) -> SystemNotification {
    SystemNotification {
        tag: tag.to_string().into(),
        title: title.to_string().into(),
        body: body.to_string().into(),
        actions: Vec::new(),
    }
}

#[gpui::test]
fn show_notification_posts_namespaced_tag_with_fields_and_actions_unchanged(
    cx: &mut TestAppContext,
) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let actions = vec![
        SystemNotificationAction {
            id: "open".into(),
            label: "Open".into(),
        },
        SystemNotificationAction {
            id: "mute".into(),
            label: "Mute".into(),
        },
    ];

    cx.update(|cx| {
        alpha.show_notification(
            SystemNotification {
                tag: "build".into(),
                title: "Build finished".into(),
                body: "3 warnings".into(),
                actions: actions.clone(),
            },
            cx,
        );
    });

    let expected = SystemNotification {
        tag: "alpha/build".into(),
        title: "Build finished".into(),
        body: "3 warnings".into(),
        actions,
    };
    assert_eq!(
        cx.shown_system_notifications(),
        vec![expected.clone()],
        "GPUI must receive exactly one notification with the namespaced tag and fields unchanged"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        vec![expected],
        "the delivered notification must match the posted one"
    );
}

#[gpui::test]
fn two_extensions_with_the_same_tag_deliver_two_distinct_notifications(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");
    let beta = extension(cx, "beta");

    cx.update(|cx| {
        alpha.show_notification(sample_notification("build", "Alpha", "From alpha"), cx);
        beta.show_notification(sample_notification("build", "Beta", "From beta"), cx);
    });

    let expected = vec![
        sample_notification("alpha/build", "Alpha", "From alpha"),
        sample_notification("beta/build", "Beta", "From beta"),
    ];
    assert_eq!(
        cx.shown_system_notifications(),
        expected,
        "both posts must reach GPUI with distinct namespaced tags"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        expected,
        "two extensions using the same tag must not overwrite each other"
    );
}

#[gpui::test]
fn reposting_the_same_tag_from_one_extension_shows_two_and_delivers_the_second(
    cx: &mut TestAppContext,
) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let first = SystemNotification {
        tag: "build".into(),
        title: "Build running".into(),
        body: "Step 1".into(),
        actions: Vec::new(),
    };
    let second = SystemNotification {
        tag: "build".into(),
        title: "Build finished".into(),
        body: "All green".into(),
        actions: vec![SystemNotificationAction {
            id: "open".into(),
            label: "Open".into(),
        }],
    };

    cx.update(|cx| {
        alpha.show_notification(first.clone(), cx);
        alpha.show_notification(second.clone(), cx);
    });

    let expected_first = SystemNotification {
        tag: "alpha/build".into(),
        ..first
    };
    let expected_second = SystemNotification {
        tag: "alpha/build".into(),
        ..second
    };
    assert_eq!(
        cx.shown_system_notifications(),
        vec![expected_first, expected_second.clone()],
        "both posts must be recorded in order"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        vec![expected_second],
        "the second post with the same tag must replace the first"
    );
}

#[gpui::test]
fn dismiss_notification_removes_only_that_extensions_notification(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");
    let beta = extension(cx, "beta");

    cx.update(|cx| {
        alpha.show_notification(sample_notification("build", "Alpha", "Alpha body"), cx);
        beta.show_notification(sample_notification("build", "Beta", "Beta body"), cx);
        alpha.dismiss_notification("build", cx);
    });

    assert_eq!(
        cx.dismissed_system_notifications(),
        vec!["alpha/build"],
        "dismiss must send the namespaced tag to GPUI"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        vec![sample_notification("beta/build", "Beta", "Beta body")],
        "dismissing alpha's tag must leave beta's notification with the same tag delivered"
    );
}

#[gpui::test]
fn responses_reach_only_the_posting_extension_with_unprefixed_tag_and_action_id(
    cx: &mut TestAppContext,
) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");
    let beta = extension(cx, "beta");

    let alpha_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));
    let beta_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));

    cx.update(|cx| {
        let sink = alpha_received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
        let sink = beta_received.clone();
        beta.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build".into(),
        action_id: None,
    });
    cx.run_until_parked();

    assert_eq!(
        *alpha_received.borrow(),
        vec![SystemNotificationResponse {
            tag: "build".into(),
            action_id: None,
        }],
        "body click must reach alpha with its own tag and action_id None once"
    );
    assert!(
        beta_received.borrow().is_empty(),
        "beta must not receive alpha's body response"
    );

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build".into(),
        action_id: Some("open".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        *alpha_received.borrow(),
        vec![
            SystemNotificationResponse {
                tag: "build".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "build".into(),
                action_id: Some("open".into()),
            },
        ],
        "action click must reach alpha with its own tag and the action id once"
    );
    assert!(
        beta_received.borrow().is_empty(),
        "beta must not receive alpha's action response"
    );

    cx.run_until_parked();
    assert_eq!(
        alpha_received.borrow().len(),
        2,
        "each response must invoke the handler exactly once"
    );
}

#[gpui::test]
fn unhandled_and_unknown_responses_run_no_handler_and_do_not_panic(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");
    let _quiet = extension(cx, "quiet");

    let alpha_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = alpha_received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
    });

    for response in [
        SystemNotificationResponse {
            tag: "quiet/build".into(),
            action_id: None,
        },
        SystemNotificationResponse {
            tag: "quiet/build".into(),
            action_id: Some("open".into()),
        },
        SystemNotificationResponse {
            tag: "ghost/x".into(),
            action_id: None,
        },
        SystemNotificationResponse {
            tag: "no-slash".into(),
            action_id: None,
        },
        SystemNotificationResponse {
            tag: "no-slash".into(),
            action_id: Some("open".into()),
        },
    ] {
        cx.simulate_system_notification_response(response);
        cx.run_until_parked();
    }

    assert!(
        alpha_received.borrow().is_empty(),
        "responses for other or unknown extensions must not invoke alpha's handler"
    );
}

#[gpui::test]
fn second_response_handler_for_the_same_extension_is_refused_and_first_stays(
    cx: &mut TestAppContext,
) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let first_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));
    let second_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));

    cx.update(|cx| {
        let sink = first_received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
        let sink = second_received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build".into(),
        action_id: None,
    });
    cx.run_until_parked();
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build".into(),
        action_id: Some("open".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        *first_received.borrow(),
        vec![
            SystemNotificationResponse {
                tag: "build".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "build".into(),
                action_id: Some("open".into()),
            },
        ],
        "the first handler must keep receiving responses"
    );
    assert!(
        second_received.borrow().is_empty(),
        "the refused second handler must never run"
    );
}

#[gpui::test]
fn response_handler_can_show_and_dismiss_notifications_on_its_extension(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    cx.update(|cx| {
        alpha.show_notification(sample_notification("initial", "Ready", "Click me"), cx);
        let handle = alpha.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, cx: &mut App| {
                handle.dismiss_notification(response.tag.as_ref(), cx);
                handle.show_notification(
                    sample_notification("followup", "Handled", "Follow-up posted"),
                    cx,
                );
            },
        );
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/initial".into(),
        action_id: Some("open".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        cx.dismissed_system_notifications(),
        vec!["alpha/initial"],
        "dismiss_notification called inside the handler must reach GPUI"
    );
    assert_eq!(
        cx.shown_system_notifications(),
        vec![
            sample_notification("alpha/initial", "Ready", "Click me"),
            sample_notification("alpha/followup", "Handled", "Follow-up posted"),
        ],
        "show_notification called inside the handler must reach GPUI"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        vec![sample_notification(
            "alpha/followup",
            "Handled",
            "Follow-up posted"
        )],
        "only the follow-up notification must remain delivered"
    );
}

#[gpui::test]
fn effects_queued_by_a_response_handler_are_flushed(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");
    let deferred_ran = Rc::new(Cell::new(false));

    cx.update(|cx| {
        let flag = deferred_ran.clone();
        alpha.on_notification_response(cx, move |_: SystemNotificationResponse, cx: &mut App| {
            let flag = flag.clone();
            cx.defer(move |_| flag.set(true));
        });
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build".into(),
        action_id: None,
    });
    cx.run_until_parked();

    assert!(
        deferred_ran.get(),
        "effects queued via cx.defer inside a response handler must be flushed"
    );
}

#[gpui::test]
fn tags_with_slashes_keep_the_extensions_tag_intact(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let received: Rc<RefCell<Vec<SystemNotificationResponse>>> = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
        alpha.show_notification(
            sample_notification("build/step1", "Build", "Step 1 finished"),
            cx,
        );
    });

    let expected = sample_notification("alpha/build/step1", "Build", "Step 1 finished");
    assert_eq!(
        cx.shown_system_notifications(),
        vec![expected.clone()],
        "GPUI must receive the tag prefixed with the extension name"
    );
    assert_eq!(
        cx.delivered_system_notifications(),
        vec![expected],
        "the delivered notification must keep the full namespaced tag"
    );

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/build/step1".into(),
        action_id: None,
    });
    cx.run_until_parked();

    assert_eq!(
        *received.borrow(),
        vec![SystemNotificationResponse {
            tag: "build/step1".into(),
            action_id: None,
        }],
        "a response tag must split at the first `/` so the extension receives `build/step1`"
    );
}

#[gpui::test]
fn registering_a_second_handler_from_inside_the_handler_is_refused(cx: &mut TestAppContext) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let first_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));
    let second_received: Rc<RefCell<Vec<SystemNotificationResponse>>> =
        Rc::new(RefCell::new(Vec::new()));

    cx.update(|cx| {
        let handle = alpha.clone();
        let first_sink = first_received.clone();
        let second_sink = second_received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, cx: &mut App| {
                first_sink.borrow_mut().push(response.clone());
                if response.tag.as_ref() == "first" {
                    let second_sink = second_sink.clone();
                    handle.on_notification_response(
                        cx,
                        move |nested: SystemNotificationResponse, _: &mut App| {
                            second_sink.borrow_mut().push(nested);
                        },
                    );
                }
            },
        );
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/first".into(),
        action_id: None,
    });
    cx.run_until_parked();

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/second".into(),
        action_id: Some("open".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        *first_received.borrow(),
        vec![
            SystemNotificationResponse {
                tag: "first".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "second".into(),
                action_id: Some("open".into()),
            },
        ],
        "a later response must still reach the first handler"
    );
    assert!(
        second_received.borrow().is_empty(),
        "a second handler registered from inside the handler must never run"
    );
}

#[gpui::test]
fn burst_of_responses_for_the_same_extension_all_reach_its_handler_in_order(
    cx: &mut TestAppContext,
) {
    set_identity(cx);
    let alpha = extension(cx, "alpha");

    let received: Rc<RefCell<Vec<SystemNotificationResponse>>> = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = received.clone();
        alpha.on_notification_response(
            cx,
            move |response: SystemNotificationResponse, _: &mut App| {
                sink.borrow_mut().push(response);
            },
        );
    });

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/first".into(),
        action_id: None,
    });
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/second".into(),
        action_id: Some("open".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        *received.borrow(),
        vec![
            SystemNotificationResponse {
                tag: "first".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "second".into(),
                action_id: Some("open".into()),
            },
        ],
        "both queued responses must reach the handler in order"
    );

    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: "alpha/third".into(),
        action_id: Some("retry".into()),
    });
    cx.run_until_parked();

    assert_eq!(
        *received.borrow(),
        vec![
            SystemNotificationResponse {
                tag: "first".into(),
                action_id: None,
            },
            SystemNotificationResponse {
                tag: "second".into(),
                action_id: Some("open".into()),
            },
            SystemNotificationResponse {
                tag: "third".into(),
                action_id: Some("retry".into()),
            },
        ],
        "the handler must still answer a third response after a burst"
    );
}
