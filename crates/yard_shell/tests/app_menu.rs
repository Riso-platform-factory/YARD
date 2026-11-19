use gpui::TestAppContext;

/// The menus the application's composition root gives the startup: the
/// application's own menu and `View` with the shell's actions.
fn menus() -> Vec<gpui::Menu> {
    vec![
        gpui::Menu::new("yard").items(vec![gpui::MenuItem::action("Quit yard", yard_shell::Quit)]),
        gpui::Menu::new("View").items(vec![
            gpui::MenuItem::action("Zoom", yard_shell::ToggleZoom),
            gpui::MenuItem::action("Hide Node", yard_shell::Hide),
            gpui::MenuItem::separator(),
            gpui::MenuItem::action("Select Next Tab", yard_shell::SelectNextTab),
            gpui::MenuItem::action("Select Previous Tab", yard_shell::SelectPreviousTab),
            gpui::MenuItem::separator(),
            gpui::MenuItem::action("Focus Left", yard_shell::FocusLeft),
            gpui::MenuItem::action("Focus Right", yard_shell::FocusRight),
            gpui::MenuItem::action("Focus Up", yard_shell::FocusUp),
            gpui::MenuItem::action("Focus Down", yard_shell::FocusDown),
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
        yard_shell::set_menus(cx, menus());
        let shown = cx.get_menus().expect("the test platform keeps the menus");
        let names: Vec<&str> = shown.iter().map(|menu| menu.name.as_ref()).collect();
        assert_eq!(names, ["yard", "View"]);
        let items: Vec<Vec<(&'static str, &str, &'static str)>> = shown
            .iter()
            .map(|menu| menu.items.iter().map(item).collect())
            .collect();
        let expected = vec![
            vec![("action", "Quit yard", "yard::Quit")],
            vec![
                ("action", "Zoom", "yard::ToggleZoom"),
                ("action", "Hide Node", "yard::Hide"),
                ("separator", "", ""),
                ("action", "Select Next Tab", "yard::SelectNextTab"),
                ("action", "Select Previous Tab", "yard::SelectPreviousTab"),
                ("separator", "", ""),
                ("action", "Focus Left", "yard::FocusLeft"),
                ("action", "Focus Right", "yard::FocusRight"),
                ("action", "Focus Up", "yard::FocusUp"),
                ("action", "Focus Down", "yard::FocusDown"),
            ],
        ];
        assert_eq!(items, expected);
    });
}

/// Asserts `yard::Quit` has exactly one binding, `keys`, with no key context.
fn quit_is_bound(cx: &mut TestAppContext, keys: &str) {
    cx.update(|cx| {
        yard_shell::bind_default_keys(cx);
        let os = cx.platform_info().os;
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bindings: Vec<_> = keymap.bindings_for_action(&yard_shell::Quit).collect();
        assert_eq!(bindings.len(), 1, "exactly one binding for yard::Quit");
        let bound: Vec<String> = bindings[0]
            .keystrokes()
            .iter()
            .map(|keystroke| keystroke.unparse(os))
            .collect();
        assert_eq!(bound, [keys]);
        assert!(
            bindings[0].predicate().is_none(),
            "the yard::Quit binding has no key context"
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
