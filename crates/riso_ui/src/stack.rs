use gpui::{Div, Styled, div};

/// A row, with its children centered across it.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// A column.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
