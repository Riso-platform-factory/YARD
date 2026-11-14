#![cfg(feature = "dev")]

use gpui::TestAppContext;

/// The menus the application's composition root gives the startup: the
/// application's own menu and `View` with the shell's actions.
fn menus() -> Vec<gpui::Menu> {
    vec![
        gpui::Menu::new("riso").items(vec![gpui::MenuItem::action(
            "Quit riso",
            riso_shell::dev::Quit,
        )]),
        gpui::Menu::new("View").items(vec![
            gpui::MenuItem::action("Zoom", riso_shell::dev::ToggleZoom),
            gpui::MenuItem::action("Hide Node", riso_shell::dev::Hide),
            gpui::MenuItem::separator(),
            gpui::MenuItem::action("Select Next Tab", riso_shell::dev::SelectNextTab),
            gpui::MenuItem::action("Select Previous Tab", riso_shell::dev::SelectPreviousTab),
            gpui::MenuItem::separator(),
            gpui::MenuItem::action("Focus Left", riso_shell::dev::FocusLeft),
            gpui::MenuItem::action("Focus Right", riso_shell::dev::FocusRight),
            gpui::MenuItem::action("Focus Up", riso_shell::dev::FocusUp),
            gpui::MenuItem::action("Focus Down", riso_shell::dev::FocusDown),
        ]),
    ]
}

/// The kind, label, and action name of one owned item.
fn item(item: &gpui::OwnedMenuItem) -> (&'static str, &str, &'static str) {
    match item {
        gpui::OwnedMenuItem::Separator => ("separator", "", ""),
        gpui::OwnedMenuItem::Action { name, action, .. } => ("action", name, action.name()),
        gpui::OwnedMenuItem::Submenu(menu) => ("submenu", menu.name.as_ref(), ""),
        gpui::OwnedMenuItem::SystemMenu(menu) => ("system", menu.name.as_ref(), ""),
    }
}

#[gpui::test]
fn set_menus_gives_gpui_the_menus_unchanged(cx: &mut TestAppContext) {
    cx.update(|cx| {
        riso_shell::dev::set_menus(cx, menus());
        let shown = cx.get_menus().expect("the test platform keeps the menus");
        let names: Vec<&str> = shown.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["riso", "View"]);
        let items: Vec<Vec<(&'static str, &str, &'static str)>> = shown
            .iter()
            .map(|menu| menu.items.iter().map(item).collect())
            .collect();
        let expected = vec![
            vec![("action", "Quit riso", "riso::Quit")],
            vec![
                ("action", "Zoom", "riso::ToggleZoom"),
                ("action", "Hide Node", "riso::Hide"),
                ("separator", "", ""),
                ("action", "Select Next Tab", "riso::SelectNextTab"),
                ("action", "Select Previous Tab", "riso::SelectPreviousTab"),
                ("separator", "", ""),
                ("action", "Focus Left", "riso::FocusLeft"),
                ("action", "Focus Right", "riso::FocusRight"),
                ("action", "Focus Up", "riso::FocusUp"),
                ("action", "Focus Down", "riso::FocusDown"),
            ],
        ];
        assert_eq!(items, expected);
    });
}

/// Asserts `riso::Quit` has exactly one binding, `keys`, with no key context.
fn quit_is_bound(cx: &mut TestAppContext, keys: &str) {
    cx.update(|cx| {
        riso_shell::dev::bind_default_keys(cx);
        let os = cx.platform_info().os;
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bindings: Vec<_> = keymap.bindings_for_action(&riso_shell::dev::Quit).collect();
        assert_eq!(bindings.len(), 1, "exactly one binding for riso::Quit");
        let bound: Vec<String> = bindings[0]
            .keystrokes()
            .iter()
            .map(|keystroke| keystroke.unparse(os))
            .collect();
        assert_eq!(bound, [keys]);
        assert!(
            bindings[0].predicate().is_none(),
            "the riso::Quit binding has no key context"
        );
    });
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn bind_default_keys_binds_quit_to_cmd_q_with_no_context(cx: &mut TestAppContext) {
    quit_is_bound(cx, "cmd-q");
}

#[cfg(not(target_os = "macos"))]
#[gpui::test]
fn bind_default_keys_binds_quit_to_ctrl_q_with_no_context(cx: &mut TestAppContext) {
    quit_is_bound(cx, "ctrl-q");
}
