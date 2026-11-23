use gpui::{Menu, MenuItem};

pub fn menus(product: &yard_product::Product) -> Vec<Menu> {
    vec![
        Menu::new(product.name).items(vec![MenuItem::action(
            format!("Quit {}", product.name),
            yard_shell::Quit,
        )]),
        Menu::new("File").items(vec![MenuItem::action("New Window", yard_shell::NewWindow)]),
        Menu::new("View").items(vec![
            MenuItem::action("Zoom", yard_shell::ToggleZoom),
            MenuItem::action("Hide Node", yard_shell::Hide),
            MenuItem::separator(),
            MenuItem::action("Select Next Tab", yard_shell::SelectNextTab),
            MenuItem::action("Select Previous Tab", yard_shell::SelectPreviousTab),
            MenuItem::separator(),
            MenuItem::action("Focus Left", yard_shell::FocusLeft),
            MenuItem::action("Focus Right", yard_shell::FocusRight),
            MenuItem::action("Focus Up", yard_shell::FocusUp),
            MenuItem::action("Focus Down", yard_shell::FocusDown),
        ]),
    ]
}
