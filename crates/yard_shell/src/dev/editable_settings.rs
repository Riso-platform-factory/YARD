use crate::Extension;
use gpui::App;
use schemars::JsonSchema;
use serde::Serialize;
use yard_settings::Settings;

impl Extension {
    /// Registers and describes the section `T`, whose key must be this extension's name.
    pub fn register_editable_settings<T: Settings + Serialize + JsonSchema>(&self, cx: &mut App) {
        if T::KEY == self.name {
            yard_settings::register_described::<T>(cx);
        } else {
            self.register_settings::<T>(cx);
        }
    }
}
