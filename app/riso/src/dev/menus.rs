use gpui::{Menu, MenuItem};

pub(crate) fn menus(product: &riso_log::Product) -> Vec<Menu> {
    vec![
        Menu::new(product.name).items(vec![MenuItem::action(
            format!("Quit {}", product.name),
            riso_shell::dev::Quit,
        )]),
        Menu::new("View").items(vec![
            MenuItem::action("Zoom", riso_shell::dev::ToggleZoom),
            MenuItem::action("Hide Node", riso_shell::dev::Hide),
            MenuItem::separator(),
            MenuItem::action("Select Next Tab", riso_shell::dev::SelectNextTab),
            MenuItem::action("Select Previous Tab", riso_shell::dev::SelectPreviousTab),
            MenuItem::separator(),
            MenuItem::action("Focus Left", riso_shell::dev::FocusLeft),
            MenuItem::action("Focus Right", riso_shell::dev::FocusRight),
            MenuItem::action("Focus Up", riso_shell::dev::FocusUp),
            MenuItem::action("Focus Down", riso_shell::dev::FocusDown),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_use_the_product_and_list_every_action_in_order() {
        let menus = menus(&riso_log::Product {
            name: "Example",
            version: "1",
        });
        assert_eq!(
            menus
                .iter()
                .map(|menu| menu.name.as_ref())
                .collect::<Vec<_>>(),
            ["Example", "View"]
        );
        assert!(menus.iter().all(|menu| !menu.disabled));
        let actual: Vec<Vec<(&str, &str)>> = menus
            .iter()
            .map(|menu| {
                menu.items
                    .iter()
                    .map(|item| match item {
                        MenuItem::Action { name, action, .. } => (name.as_ref(), action.name()),
                        MenuItem::Separator => ("", ""),
                        _ => panic!("unexpected menu item"),
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                vec![("Quit Example", "riso::Quit")],
                vec![
                    ("Zoom", "riso::ToggleZoom"),
                    ("Hide Node", "riso::Hide"),
                    ("", ""),
                    ("Select Next Tab", "riso::SelectNextTab"),
                    ("Select Previous Tab", "riso::SelectPreviousTab"),
                    ("", ""),
                    ("Focus Left", "riso::FocusLeft"),
                    ("Focus Right", "riso::FocusRight"),
                    ("Focus Up", "riso::FocusUp"),
                    ("Focus Down", "riso::FocusDown"),
                ]
            ]
        );
    }
}
