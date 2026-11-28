use gpui::{
    AnyWindowHandle, Menu, MenuItem, OsAction, SystemMenuType, TestAppContext, VisualTestContext,
};
use image::{ImageFormat, Rgba, RgbaImage};
use std::{
    cell::{Cell, RefCell},
    io::Cursor,
    rc::Rc,
};
use yard_shell::{
    self as shell, Extension, Tray, TrayBackend, TrayError, TrayIcon, TrayMenuItemSnapshot,
};

gpui::actions!(
    notes,
    [
        NewNote,
        TogglePin,
        DisabledAction,
        InnerAction,
        FirstItem,
        SecondItem,
        CopyNote,
    ]
);

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .expect("encoding test PNG must succeed");
    bytes
}

fn open_shell_window(cx: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = cx.update(|cx| {
        cx.open_window(gpui::WindowOptions::default(), shell::new_shell)
            .unwrap()
    });
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(640.), gpui::px(480.)));
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn open_active_shell_window(cx: &mut TestAppContext) -> gpui::WindowHandle<shell::Shell> {
    let window = open_shell_window(cx);
    gpui::AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.activate_window();
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

#[gpui::test]
fn set_update_remove_and_error_display(cx: &mut TestAppContext) {
    let errors = [
        (
            TrayError::AlreadyOwned,
            "another extension owns the tray icon",
        ),
        (TrayError::InvalidIcon, "tray icon is not a valid PNG"),
        (TrayError::Unavailable, "no tray on this desktop"),
        (TrayError::Failed, "tray icon failed"),
    ];
    for (error, expected) in errors {
        assert_eq!(error.to_string(), expected);
    }

    let backend = TrayBackend::memory();
    let notes = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.icon_template(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());

    let icon_16 = png_bytes(16, 16);
    cx.update(|cx| {
        notes.set_tray(
            Tray::new(TrayIcon::png(&icon_16).template(true))
                .tooltip("Notes is running")
                .menu(vec![
                    MenuItem::action("New Note", NewNote),
                    MenuItem::action("Pinned", TogglePin).checked(true),
                    MenuItem::action("Unavailable", DisabledAction).disabled(true),
                    MenuItem::separator(),
                    MenuItem::submenu(
                        Menu::new("More").items(vec![MenuItem::action("Inner", InnerAction)]),
                    ),
                ]),
            cx,
        )
    })
    .unwrap();

    assert_eq!(backend.icon_size(), Some((16, 16)));
    assert_eq!(backend.icon_template(), Some(true));
    assert_eq!(backend.tooltip().as_deref(), Some("Notes is running"));
    assert_eq!(
        backend.menu_snapshot(),
        vec![
            TrayMenuItemSnapshot::Action {
                label: "New Note".into(),
                action_name: "notes::NewNote".into(),
                checked: false,
                disabled: false,
            },
            TrayMenuItemSnapshot::Action {
                label: "Pinned".into(),
                action_name: "notes::TogglePin".into(),
                checked: true,
                disabled: false,
            },
            TrayMenuItemSnapshot::Action {
                label: "Unavailable".into(),
                action_name: "notes::DisabledAction".into(),
                checked: false,
                disabled: true,
            },
            TrayMenuItemSnapshot::Separator,
            TrayMenuItemSnapshot::Submenu {
                label: "More".into(),
                disabled: false,
                items: vec![TrayMenuItemSnapshot::Action {
                    label: "Inner".into(),
                    action_name: "notes::InnerAction".into(),
                    checked: false,
                    disabled: false,
                }],
            },
        ]
    );

    // Updating the tray replaces icon, template flag, tooltip, and menu.
    let icon_32x24 = png_bytes(32, 24);
    cx.update(|cx| {
        notes.set_tray(
            Tray::new(TrayIcon::png(&icon_32x24))
                .tooltip("Notes updated")
                .menu(vec![MenuItem::action("Quit", shell::Quit)]),
            cx,
        )
    })
    .unwrap();

    assert_eq!(backend.icon_size(), Some((32, 24)));
    assert_eq!(backend.icon_template(), Some(false));
    assert_eq!(backend.tooltip().as_deref(), Some("Notes updated"));
    assert_eq!(
        backend.menu_snapshot(),
        vec![TrayMenuItemSnapshot::Action {
            label: "Quit".into(),
            action_name: "yard::Quit".into(),
            checked: false,
            disabled: false,
        }]
    );

    // Removing the tray clears the icon, tooltip, and menu.
    cx.update(|cx| notes.remove_tray(cx));
    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.icon_template(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());

    // Removing again when not owning does nothing.
    cx.update(|cx| notes.remove_tray(cx));
    assert_eq!(backend.icon_size(), None);
}

#[gpui::test]
fn second_extension_gets_already_owned_until_owner_removes(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let (alpha, beta) = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let alpha = Extension::new(cx, "alpha").unwrap();
        let beta = Extension::new(cx, "beta").unwrap();
        (alpha, beta)
    });

    let alpha_icon = png_bytes(16, 16);
    let beta_icon = png_bytes(24, 24);

    cx.update(|cx| {
        alpha.set_tray(
            Tray::new(TrayIcon::png(&alpha_icon).template(true))
                .tooltip("Alpha tray")
                .menu(vec![MenuItem::action("Quit", shell::Quit)]),
            cx,
        )
    })
    .unwrap();

    let refused = cx.update(|cx| {
        beta.set_tray(
            Tray::new(TrayIcon::png(&beta_icon)).tooltip("Beta tray"),
            cx,
        )
    });
    assert_eq!(
        refused,
        Err(TrayError::AlreadyOwned),
        "a second extension must get AlreadyOwned while the first owns the tray"
    );

    // Alpha's icon, tooltip, and menu must be unchanged.
    assert_eq!(backend.icon_size(), Some((16, 16)));
    assert_eq!(backend.icon_template(), Some(true));
    assert_eq!(backend.tooltip().as_deref(), Some("Alpha tray"));
    assert_eq!(backend.menu_snapshot().len(), 1);

    // Removing from the non-owner does nothing.
    cx.update(|cx| beta.remove_tray(cx));
    assert_eq!(backend.icon_size(), Some((16, 16)));
    assert_eq!(backend.tooltip().as_deref(), Some("Alpha tray"));

    // After the owner removes the tray, the second extension can take it.
    cx.update(|cx| alpha.remove_tray(cx));
    assert_eq!(backend.icon_size(), None);

    cx.update(|cx| {
        beta.set_tray(
            Tray::new(TrayIcon::png(&beta_icon)).tooltip("Beta tray"),
            cx,
        )
    })
    .unwrap();
    assert_eq!(backend.icon_size(), Some((24, 24)));
    assert_eq!(backend.icon_template(), Some(false));
    assert_eq!(backend.tooltip().as_deref(), Some("Beta tray"));
    assert!(backend.menu_snapshot().is_empty());
}

#[gpui::test]
fn invalid_icon_rejected_and_256x256_accepted(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png([])), cx)),
        Err(TrayError::InvalidIcon),
        "empty bytes must be refused with InvalidIcon"
    );
    assert_eq!(backend.icon_size(), None);

    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(b"not a png image")), cx)),
        Err(TrayError::InvalidIcon),
        "non-PNG bytes must be refused with InvalidIcon"
    );
    assert_eq!(backend.icon_size(), None);

    let wide = png_bytes(257, 1);
    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&wide)), cx)),
        Err(TrayError::InvalidIcon),
        "257x1 PNG must be refused with InvalidIcon"
    );
    assert_eq!(backend.icon_size(), None);

    let tall = png_bytes(1, 257);
    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&tall)), cx)),
        Err(TrayError::InvalidIcon),
        "1x257 PNG must be refused with InvalidIcon"
    );
    assert_eq!(backend.icon_size(), None);

    let max = png_bytes(256, 256);
    cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&max)), cx))
        .unwrap();
    assert_eq!(backend.icon_size(), Some((256, 256)));
}

#[gpui::test]
fn simulate_menu_dispatches_top_level_and_submenu_actions_to_active_window(
    cx: &mut TestAppContext,
) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let new_notes = Rc::new(Cell::new(0usize));
    let inner_clicks = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let new_notes = new_notes.clone();
        ext.on_action::<NewNote>(cx, move |_, _, _| {
            new_notes.set(new_notes.get() + 1);
        });
        let inner_clicks = inner_clicks.clone();
        ext.on_action::<InnerAction>(cx, move |_, _, _| {
            inner_clicks.set(inner_clicks.get() + 1);
        });
        ext
    });

    let _window = open_active_shell_window(cx);

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![
                MenuItem::action("New Note", NewNote),
                MenuItem::separator(),
                MenuItem::submenu(
                    Menu::new("More").items(vec![MenuItem::action("Inner", InnerAction)]),
                ),
            ]),
            cx,
        )
    })
    .unwrap();

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        new_notes.get(),
        1,
        "top-level menu item must dispatch NewNote to the active window"
    );
    assert_eq!(inner_clicks.get(), 0);

    backend.simulate_menu(&[2, 0]);
    cx.run_until_parked();
    assert_eq!(new_notes.get(), 1);
    assert_eq!(
        inner_clicks.get(),
        1,
        "submenu inner item must dispatch InnerAction to the active window"
    );
}

#[gpui::test]
fn simulate_menu_with_no_window_dispatches_to_application(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let quits = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let quits = quits.clone();
        cx.on_action(move |_: &shell::Quit, _| {
            quits.set(quits.get() + 1);
        });
        Extension::new(cx, "notes").unwrap()
    });

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("Quit", shell::Quit)]),
            cx,
        )
    })
    .unwrap();

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        quits.get(),
        1,
        "with no window open, menu item must dispatch to the application handler"
    );
}

#[gpui::test]
fn simulate_activate_runs_handler_replaces_previous_and_ignores_missing(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    let icon = png_bytes(16, 16);
    cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon)), cx))
        .unwrap();

    // With no handler registered, simulating activation does not panic.
    backend.simulate_activate();
    cx.run_until_parked();

    let first = Rc::new(Cell::new(0usize));
    let second = Rc::new(Cell::new(0usize));

    cx.update(|cx| {
        let first = first.clone();
        ext.on_tray_activate(cx, move |_| {
            first.set(first.get() + 1);
        });
    });

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 0);

    // A second registration replaces the first.
    cx.update(|cx| {
        let second = second.clone();
        ext.on_tray_activate(cx, move |_| {
            second.set(second.get() + 1);
        });
    });

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(
        first.get(),
        1,
        "replaced activation handler must not run again"
    );
    assert_eq!(second.get(), 1, "new activation handler must run");
}

#[gpui::test]
fn fail_next_injects_unavailable_and_failed_once_then_recovers(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    let icon_16 = png_bytes(16, 16);
    let icon_32 = png_bytes(32, 32);

    backend.fail_next(TrayError::Unavailable);
    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon_16)), cx)),
        Err(TrayError::Unavailable),
        "fail_next(Unavailable) must make the next set_tray return Unavailable"
    );
    assert_eq!(backend.icon_size(), None);

    cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon_16)), cx))
        .unwrap();
    assert_eq!(backend.icon_size(), Some((16, 16)));

    backend.fail_next(TrayError::Failed);
    assert_eq!(
        cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon_32)), cx)),
        Err(TrayError::Failed),
        "fail_next(Failed) must make the next set_tray return Failed"
    );
    assert_eq!(
        backend.icon_size(),
        Some((16, 16)),
        "a failed update must leave the previous tray icon intact"
    );

    cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon_32)), cx))
        .unwrap();
    assert_eq!(backend.icon_size(), Some((32, 32)));
}

#[gpui::test]
fn unsupported_menu_item_kind_is_omitted_from_snapshot(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let first_clicks = Rc::new(Cell::new(0usize));
    let second_clicks = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let first_clicks = first_clicks.clone();
        ext.on_action::<FirstItem>(cx, move |_, _, _| {
            first_clicks.set(first_clicks.get() + 1);
        });
        let second_clicks = second_clicks.clone();
        ext.on_action::<SecondItem>(cx, move |_, _, _| {
            second_clicks.set(second_clicks.get() + 1);
        });
        ext
    });

    let _window = open_active_shell_window(cx);

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![
                MenuItem::action("First", FirstItem),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::action("Second", SecondItem),
            ]),
            cx,
        )
    })
    .unwrap();

    assert_eq!(
        backend.menu_snapshot(),
        vec![
            TrayMenuItemSnapshot::Action {
                label: "First".into(),
                action_name: "notes::FirstItem".into(),
                checked: false,
                disabled: false,
            },
            TrayMenuItemSnapshot::Action {
                label: "Second".into(),
                action_name: "notes::SecondItem".into(),
                checked: false,
                disabled: false,
            },
        ]
    );

    // Omitting os_submenu shifts "Second" from input index 2 to snapshot index 1.
    backend.simulate_menu(&[1]);
    cx.run_until_parked();
    assert_eq!(first_clicks.get(), 0);
    assert_eq!(
        second_clicks.get(),
        1,
        "simulate_menu on the shifted index must run the action after the omitted os_submenu"
    );

    // The unshifted index 2 no longer refers to an item.
    backend.simulate_menu(&[2]);
    cx.run_until_parked();
    assert_eq!(first_clicks.get(), 0);
    assert_eq!(second_clicks.get(), 1);
}

#[gpui::test]
fn routing_prefers_active_window_then_first_window_then_application(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let window_hits: Rc<RefCell<Vec<AnyWindowHandle>>> = Rc::new(RefCell::new(Vec::new()));
    let app_hits = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let app_hits = app_hits.clone();
        cx.on_action(move |_: &NewNote, _| {
            app_hits.set(app_hits.get() + 1);
        });
        let ext = Extension::new(cx, "notes").unwrap();
        let window_hits = window_hits.clone();
        ext.on_action::<NewNote>(cx, move |_, window, _| {
            window_hits.borrow_mut().push(window.window_handle());
        });
        ext
    });

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("New Note", NewNote)]),
            cx,
        )
    })
    .unwrap();

    let first_window = open_shell_window(cx);
    let second_window = open_shell_window(cx);
    let first_handle: AnyWindowHandle = first_window.into();
    let second_handle: AnyWindowHandle = second_window.into();
    assert_ne!(first_handle, second_handle);
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(
        cx.update(|cx| cx.windows()),
        vec![first_handle, second_handle]
    );

    // With no window ever activated, the first open window receives the action.
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![first_handle],
        "with no window ever activated, the first open window must receive the action"
    );
    assert_eq!(app_hits.get(), 0);

    // Active window (the second one) receives the action, not the first.
    second_handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(second_handle));

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(*window_hits.borrow(), vec![first_handle, second_handle]);
    assert_eq!(app_hits.get(), 0);

    // Switching active window to the first routes there.
    first_handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(first_handle));

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![first_handle, second_handle, first_handle]
    );
    assert_eq!(app_hits.get(), 0);

    // Deactivate and close both windows; with no window open, the application receives it.
    let mut first_visual = VisualTestContext::from_window(first_handle, cx);
    first_visual.deactivate_window();
    drop(first_visual);
    first_handle
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    second_handle
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert!(cx.update(|cx| cx.windows().is_empty()));

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![first_handle, second_handle, first_handle]
    );
    assert_eq!(
        app_hits.get(),
        1,
        "with no window open, the application must receive the action"
    );
}

#[gpui::test]
fn routing_uses_most_recently_active_window_and_falls_back_on_close(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let window_hits: Rc<RefCell<Vec<AnyWindowHandle>>> = Rc::new(RefCell::new(Vec::new()));
    let app_hits = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let app_hits = app_hits.clone();
        cx.on_action(move |_: &NewNote, _| {
            app_hits.set(app_hits.get() + 1);
        });
        let ext = Extension::new(cx, "notes").unwrap();
        let window_hits = window_hits.clone();
        ext.on_action::<NewNote>(cx, move |_, window, _| {
            window_hits.borrow_mut().push(window.window_handle());
        });
        ext
    });

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("New Note", NewNote)]),
            cx,
        )
    })
    .unwrap();

    let window_a = open_shell_window(cx);
    let window_b = open_shell_window(cx);
    let handle_a: AnyWindowHandle = window_a.into();
    let handle_b: AnyWindowHandle = window_b.into();
    assert_ne!(handle_a, handle_b);

    // Activate A, then B, then leave no window active.
    handle_a
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(handle_a));

    handle_b
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), Some(handle_b));

    let mut visual_b = VisualTestContext::from_window(handle_b, cx);
    visual_b.deactivate_window();
    drop(visual_b);
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![handle_a, handle_b]);

    // With no active window, B (most recently active) receives the action, not A (first open).
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_b],
        "with no active window, the most recently active window (B) must receive the action"
    );
    assert_eq!(app_hits.get(), 0);

    // Close B; with no active window, A receives the next menu action as the last active window still open.
    handle_b
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![handle_a]);

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_b, handle_a],
        "after closing B, A must receive the action as the last active window still open"
    );
    assert_eq!(app_hits.get(), 0);
}

#[gpui::test]
fn closing_last_active_window_falls_back_to_prior_active_before_first_window(
    cx: &mut TestAppContext,
) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let window_hits: Rc<RefCell<Vec<AnyWindowHandle>>> = Rc::new(RefCell::new(Vec::new()));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let window_hits = window_hits.clone();
        ext.on_action::<NewNote>(cx, move |_, window, _| {
            window_hits.borrow_mut().push(window.window_handle());
        });
        ext
    });

    let icon = png_bytes(16, 16);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("New Note", NewNote)]),
            cx,
        )
    })
    .unwrap();

    // Open an unactivated first window, then A and B.
    let idle_handle: AnyWindowHandle = open_shell_window(cx).into();
    let handle_a: AnyWindowHandle = open_shell_window(cx).into();
    let handle_b: AnyWindowHandle = open_shell_window(cx).into();

    handle_a
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    handle_b
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();

    let mut visual_b = VisualTestContext::from_window(handle_b, cx);
    visual_b.deactivate_window();
    drop(visual_b);
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(
        cx.update(|cx| cx.windows()),
        vec![idle_handle, handle_a, handle_b]
    );

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(*window_hits.borrow(), vec![handle_b]);

    // Closing B must route to A (prior active), not idle_handle (first open).
    handle_b
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![idle_handle, handle_a]);

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_b, handle_a],
        "after closing B, the previously active window A must take precedence over the unactivated first window"
    );

    // Closing A leaves only the never-activated first window, so routing falls back to it.
    handle_a
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert_eq!(cx.update(|cx| cx.windows()), vec![idle_handle]);

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        *window_hits.borrow(),
        vec![handle_b, handle_a, idle_handle],
        "once every previously active window is closed, routing must fall back to the first open window"
    );
}

#[gpui::test]
fn stale_menu_event_queued_before_set_tray_is_ignored(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let first_clicks = Rc::new(Cell::new(0usize));
    let second_clicks = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let first_clicks = first_clicks.clone();
        ext.on_action::<FirstItem>(cx, move |_, _, _| {
            first_clicks.set(first_clicks.get() + 1);
        });
        let second_clicks = second_clicks.clone();
        ext.on_action::<SecondItem>(cx, move |_, _, _| {
            second_clicks.set(second_clicks.get() + 1);
        });
        ext
    });

    let _window = open_active_shell_window(cx);
    let icon = png_bytes(16, 16);

    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("First", FirstItem)]),
            cx,
        )
    })
    .unwrap();

    // Queue a menu click against the first menu, then replace the menu before draining.
    backend.simulate_menu(&[0]);
    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::action("Second", SecondItem)]),
            cx,
        )
    })
    .unwrap();

    cx.run_until_parked();
    assert_eq!(
        first_clicks.get(),
        0,
        "stale menu event must not run the old action"
    );
    assert_eq!(
        second_clicks.get(),
        0,
        "stale menu event must not run the new action at the same index"
    );

    // A fresh click against the updated menu runs the new action.
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(first_clicks.get(), 0);
    assert_eq!(second_clicks.get(), 1);
}

#[gpui::test]
fn handlers_can_update_and_remove_tray_without_panicking_or_deadlocking(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let icon_16 = png_bytes(16, 16);
    let icon_24 = png_bytes(24, 24);
    let icon_32 = png_bytes(32, 32);

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });

    // Activation handler calls set_tray.
    cx.update(|cx| {
        let ext_for_activate = ext.clone();
        let icon_24 = icon_24.clone();
        ext.on_tray_activate(cx, move |cx| {
            ext_for_activate
                .set_tray(
                    Tray::new(TrayIcon::png(&icon_24))
                        .tooltip("From activate")
                        .menu(vec![
                            MenuItem::action("Update from menu", FirstItem),
                            MenuItem::action("Remove from menu", SecondItem),
                        ]),
                    cx,
                )
                .unwrap();
        });

        let ext_for_update = ext.clone();
        let icon_32 = icon_32.clone();
        ext.on_action::<FirstItem>(cx, move |_, _, cx| {
            ext_for_update
                .set_tray(
                    Tray::new(TrayIcon::png(&icon_32))
                        .tooltip("From menu")
                        .menu(vec![MenuItem::action("Remove from menu", SecondItem)]),
                    cx,
                )
                .unwrap();
        });

        let ext_for_remove = ext.clone();
        ext.on_action::<SecondItem>(cx, move |_, _, cx| {
            ext_for_remove.remove_tray(cx);
        });
    });

    let window = open_active_shell_window(cx);

    cx.update(|cx| ext.set_tray(Tray::new(TrayIcon::png(&icon_16)).tooltip("Initial"), cx))
        .unwrap();

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), Some((24, 24)));
    assert_eq!(backend.tooltip().as_deref(), Some("From activate"));
    assert_eq!(backend.menu_snapshot().len(), 2);

    // Menu action (window) calls set_tray.
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), Some((32, 32)));
    assert_eq!(backend.tooltip().as_deref(), Some("From menu"));
    assert_eq!(backend.menu_snapshot().len(), 1);

    // Menu action (window) calls remove_tray.
    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());

    // Deactivate and close the window so menu actions run synchronously on the app.
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.deactivate_window();
    drop(visual);
    AnyWindowHandle::from(window)
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| cx.active_window()), None);
    assert!(cx.update(|cx| cx.windows().is_empty()));

    cx.update(|cx| {
        let ext_for_app_update = ext.clone();
        let icon_24 = icon_24.clone();
        cx.on_action(move |_: &FirstItem, cx| {
            ext_for_app_update
                .set_tray(
                    Tray::new(TrayIcon::png(&icon_24))
                        .tooltip("From app menu")
                        .menu(vec![MenuItem::action("Remove from app", SecondItem)]),
                    cx,
                )
                .unwrap();
        });
        let ext_for_app_remove = ext.clone();
        cx.on_action(move |_: &SecondItem, cx| {
            ext_for_app_remove.remove_tray(cx);
        });
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon_16))
                .tooltip("Before app menu")
                .menu(vec![MenuItem::action("Update from app", FirstItem)]),
            cx,
        )
    })
    .unwrap();

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), Some((24, 24)));
    assert_eq!(backend.tooltip().as_deref(), Some("From app menu"));
    assert_eq!(backend.menu_snapshot().len(), 1);

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());

    // Activation handler calls remove_tray.
    cx.update(|cx| {
        let ext_for_remove = ext.clone();
        ext.on_tray_activate(cx, move |cx| {
            ext_for_remove.remove_tray(cx);
        });
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon_16)).tooltip("Before remove"),
            cx,
        )
    })
    .unwrap();

    backend.simulate_activate();
    cx.run_until_parked();
    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());
}

#[gpui::test]
fn app_quit_clears_tray_icon(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let backend = TrayBackend::memory();
    let icon = png_bytes(16, 16);

    let ext = app.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });
    app.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon))
                .tooltip("Notes")
                .menu(vec![MenuItem::action("Quit", shell::Quit)]),
            cx,
        )
    })
    .unwrap();
    assert_eq!(backend.icon_size(), Some((16, 16)));

    app.quit();
    app.run_until_parked();

    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());
}

#[gpui::test]
fn quit_observer_after_tray_gets_unavailable_and_leaves_no_icon(cx: &mut TestAppContext) {
    let app = cx.new_app();
    let backend = TrayBackend::memory();
    let icon = png_bytes(16, 16);
    let late_result = Rc::new(Cell::new(None));

    let ext = app.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        Extension::new(cx, "notes").unwrap()
    });
    app.update(|cx| {
        ext.set_tray(Tray::new(TrayIcon::png(&icon)).tooltip("Notes"), cx)
            .unwrap();
        let late_result = late_result.clone();
        let ext = ext.clone();
        let icon = icon.clone();
        cx.on_app_quit(move |cx| {
            late_result.set(Some(
                ext.set_tray(Tray::new(TrayIcon::png(&icon)).tooltip("Late"), cx),
            ));
            async {}
        })
        .detach();
    });
    assert_eq!(backend.icon_size(), Some((16, 16)));

    app.quit();
    app.run_until_parked();

    assert_eq!(
        late_result.get(),
        Some(Err(TrayError::Unavailable)),
        "a quit observer registered after the tray must get Unavailable from set_tray"
    );
    assert_eq!(
        backend.icon_size(),
        None,
        "no tray icon may be left after quit"
    );
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());
}

#[gpui::test]
fn dropping_app_clears_tray_icon(cx: &mut TestAppContext) {
    let backend = TrayBackend::memory();
    let icon = png_bytes(16, 16);

    {
        let app = cx.new_app();
        let ext = app.update(|cx| {
            shell::set_tray_backend(cx, backend.clone());
            Extension::new(cx, "notes").unwrap()
        });
        app.update(|cx| {
            ext.set_tray(
                Tray::new(TrayIcon::png(&icon))
                    .tooltip("Notes")
                    .menu(vec![MenuItem::action("Quit", shell::Quit)]),
                cx,
            )
        })
        .unwrap();
        assert_eq!(backend.icon_size(), Some((16, 16)));
    }

    assert_eq!(backend.icon_size(), None);
    assert_eq!(backend.tooltip(), None);
    assert!(backend.menu_snapshot().is_empty());
}

#[gpui::test]
fn disabled_menu_item_appears_in_snapshot_but_dispatches_nothing(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let disabled_clicks = Rc::new(Cell::new(0usize));
    let inner_clicks = Rc::new(Cell::new(0usize));
    let enabled_clicks = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let disabled_clicks = disabled_clicks.clone();
        ext.on_action::<DisabledAction>(cx, move |_, _, _| {
            disabled_clicks.set(disabled_clicks.get() + 1);
        });
        let inner_clicks = inner_clicks.clone();
        ext.on_action::<InnerAction>(cx, move |_, _, _| {
            inner_clicks.set(inner_clicks.get() + 1);
        });
        let enabled_clicks = enabled_clicks.clone();
        ext.on_action::<FirstItem>(cx, move |_, _, _| {
            enabled_clicks.set(enabled_clicks.get() + 1);
        });
        ext
    });

    let _window = open_active_shell_window(cx);
    let icon = png_bytes(16, 16);

    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![
                MenuItem::action("Disabled", DisabledAction).disabled(true),
                MenuItem::submenu(
                    Menu::new("Disabled Sub")
                        .disabled(true)
                        .items(vec![MenuItem::action("Inner", InnerAction)]),
                ),
                MenuItem::action("Enabled", FirstItem),
            ]),
            cx,
        )
    })
    .unwrap();

    assert_eq!(
        backend.menu_snapshot(),
        vec![
            TrayMenuItemSnapshot::Action {
                label: "Disabled".into(),
                action_name: "notes::DisabledAction".into(),
                checked: false,
                disabled: true,
            },
            TrayMenuItemSnapshot::Submenu {
                label: "Disabled Sub".into(),
                disabled: true,
                items: vec![TrayMenuItemSnapshot::Action {
                    label: "Inner".into(),
                    action_name: "notes::InnerAction".into(),
                    checked: false,
                    disabled: false,
                }],
            },
            TrayMenuItemSnapshot::Action {
                label: "Enabled".into(),
                action_name: "notes::FirstItem".into(),
                checked: false,
                disabled: false,
            },
        ]
    );

    backend.simulate_menu(&[0]);
    backend.simulate_menu(&[1, 0]);
    cx.run_until_parked();
    assert_eq!(
        disabled_clicks.get(),
        0,
        "simulate_menu on a disabled action must dispatch nothing"
    );
    assert_eq!(
        inner_clicks.get(),
        0,
        "simulate_menu inside a disabled submenu must dispatch nothing"
    );

    backend.simulate_menu(&[2]);
    cx.run_until_parked();
    assert_eq!(enabled_clicks.get(), 1);
}

#[gpui::test]
fn os_action_menu_item_dispatches_like_regular_action(cx: &mut TestAppContext) {
    cx.update(yard_ui::init);
    let backend = TrayBackend::memory();
    let copies = Rc::new(Cell::new(0usize));

    let ext = cx.update(|cx| {
        shell::set_tray_backend(cx, backend.clone());
        let ext = Extension::new(cx, "notes").unwrap();
        let copies = copies.clone();
        ext.on_action::<CopyNote>(cx, move |_, _, _| {
            copies.set(copies.get() + 1);
        });
        ext
    });

    let _window = open_active_shell_window(cx);
    let icon = png_bytes(16, 16);

    cx.update(|cx| {
        ext.set_tray(
            Tray::new(TrayIcon::png(&icon)).menu(vec![MenuItem::os_action(
                "Copy",
                CopyNote,
                OsAction::Copy,
            )]),
            cx,
        )
    })
    .unwrap();

    assert_eq!(
        backend.menu_snapshot(),
        vec![TrayMenuItemSnapshot::Action {
            label: "Copy".into(),
            action_name: "notes::CopyNote".into(),
            checked: false,
            disabled: false,
        }]
    );

    backend.simulate_menu(&[0]);
    cx.run_until_parked();
    assert_eq!(
        copies.get(),
        1,
        "an os_action menu item must dispatch its action like a regular action"
    );
}
