use super::tab_close_label;
use gpui::SharedString;

#[test]
fn tab_close_label_names_the_tabs_close_button() {
    let notes: SharedString = tab_close_label("Notes");
    assert_eq!(notes.as_ref(), "Close Notes");
    assert_eq!(tab_close_label("Settings").as_ref(), "Close Settings");
    assert_eq!(tab_close_label("My Tab").as_ref(), "Close My Tab");
}
