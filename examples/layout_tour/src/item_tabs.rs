use super::*;
use gpui::SharedString;

/// Closes an item by removing its key, except `ada`, which it keeps, and
/// makes a new item `new-<n>` with the smallest free `n`.
struct Names(Vec<SharedString>);

impl shell::Items for Names {
    fn items(&self) -> Vec<SharedString> {
        self.0.clone()
    }
}

impl shell::ItemTabs for Names {
    fn close_item(&mut self, key: &SharedString, _: &mut Window, cx: &mut Context<Self>) {
        if key.as_str() == "ada" {
            tracing::debug!(key = key.as_str(), "names keep the item");
        } else {
            self.0.retain(|name| name != key);
            tracing::debug!(key = key.as_str(), "names close the item");
        }
        cx.notify();
    }

    fn new_item(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let n = (1..)
            .find(|n| {
                !self
                    .0
                    .iter()
                    .any(|name| name.as_str() == format!("new-{n}"))
            })
            .expect("a free number");
        let key = SharedString::from(format!("new-{n}"));
        tracing::debug!(key = key.as_str(), "names add an item");
        self.0.push(key);
        cx.notify();
    }
}

/// A card in item tabs is titled by its item's key (feature 14).
impl shell::Titled for Card {
    fn title(&self, cx: &App) -> SharedString {
        shell::item(cx, &self.node).unwrap_or_else(|| self.node.clone().into())
    }
}

pub(super) fn init(cx: &mut App, ext: &shell::Extension) {
    register_card(cx, ext);
    ext.register_title::<Card>(cx, "layout_tour.card");
    ext.provide::<Names>(cx, "layout_tour.names", |_, cx| {
        cx.new(|_| Names(vec!["ada".into(), "bob".into(), "cy".into()]))
    });
    ext.register(cx, "layout_tour.names", &["children"], |_, slots, _, cx| {
        cx.new(|_| NamesView { slots }).into()
    });
    place_root(ext, cx, "root", "layout_tour.names");
    ext.place(cx, "names_tabs", "riso.tabs", "root", "children");
    ext.item_tabs::<Names>(cx, "names_tabs");
    ext.place(cx, "name", "layout_tour.card", "names_tabs", "children");
}

struct NamesView {
    slots: shell::Slots,
}

impl Render for NamesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering names");
        riso_ui::v_flex()
            .size_full()
            .gap_2()
            .p_2()
            .child(riso_ui::Label::new(format!(
                "user layer: {}",
                shell::user_layer_len(cx)
            )))
            .children(
                self.slots
                    .views("children")
                    .into_iter()
                    .map(|view| div().flex_1().min_h_0().child(view)),
            )
    }
}
