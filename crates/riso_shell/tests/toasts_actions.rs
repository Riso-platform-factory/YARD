use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext};
use riso_shell as shell;
use riso_ui::{Toast, ToastKind};
use std::{cell::Cell, rc::Rc};

#[gpui::test]
fn toast_actions_can_replace_or_hide_the_toast_without_borrowing_the_layer(
    cx: &mut TestAppContext,
) {
    cx.update(riso_ui::init);
    let ext = cx.update(|cx| shell::Extension::new(cx, "actions").unwrap());
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), shell::new_shell)
            .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let replaced_handler = Rc::new(Cell::new(0));
    let old = replaced_handler.clone();
    let replace_ext = ext.clone();
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            ext.show_toast(
                Toast::new(ToastKind::Info, "first")
                    .action("old", move |_, _| old.set(old.get() + 1))
                    .action("replace", move |window, cx| {
                        replace_ext.show_toast(
                            Toast::new(ToastKind::Warning, "replacement"),
                            window,
                            cx,
                        )
                    }),
                window,
                cx,
            );
            window.refresh();
            let _ = window.draw(cx);
        })
        .unwrap();
    let action = visual.debug_bounds("TOAST-ACTION").unwrap();
    visual.simulate_click(action.center(), Default::default());
    visual.update(|window, cx| {
        assert!(shell::has_toast(window, cx));
        window.refresh();
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("TOAST-ACTION").is_none());
    assert_eq!(replaced_handler.get(), 0);

    let hide_ext = ext.clone();
    visual.update(|window, cx| {
        ext.show_toast(
            Toast::new(ToastKind::Error, "hide")
                .action("hide", move |window, cx| hide_ext.hide_toast(window, cx)),
            window,
            cx,
        );
        window.refresh();
        let _ = window.draw(cx);
    });
    let action = visual.debug_bounds("TOAST-ACTION").unwrap();
    visual.simulate_click(action.center(), Default::default());
    visual.update(|window, cx| assert!(!shell::has_toast(window, cx)));
}
