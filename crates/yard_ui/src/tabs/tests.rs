use super::{Tab, TabBar, tab_close_label};
use gpui::{Element, Role, SharedString, TestAppContext, accesskit, div};

#[test]
fn tab_close_label_names_the_tabs_close_button() {
    let notes: SharedString = tab_close_label("Notes");
    assert_eq!(notes.as_ref(), "Close Notes");
    assert_eq!(tab_close_label("Settings").as_ref(), "Close Settings");
    assert_eq!(tab_close_label("My Tab").as_ref(), "Close My Tab");
}

#[gpui::test]
fn tab_list_exposes_the_number_of_tabs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for count in [0, 1, 3] {
            let tabs = (0..count).map(|index| Tab::new(index, "Tab")).collect();
            let list = TabBar::new("tabs")
                .tabs(tabs)
                .end_child(div())
                .element(false, cx);

            assert_eq!(list.a11y_role(), Some(Role::TabList));
            let mut node = accesskit::Node::new(Role::TabList);
            list.write_a11y_info(&mut node);
            assert_eq!(node.size_of_set(), Some(count));
        }
    });
}
