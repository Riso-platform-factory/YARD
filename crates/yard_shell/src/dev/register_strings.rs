use crate::Extension;
use gpui::App;

impl Extension {
    /// Registers or replaces this extension's catalog for a language.
    pub fn register_strings(
        &self,
        cx: &mut App,
        locale: &str,
        json: &str,
    ) -> Result<(), yard_i18n::CatalogError> {
        if self.name == "app" || self.name.starts_with("yard_") {
            tracing::info!(namespace = %self.name, locale, "catalog namespace is reserved");
            return Err(yard_i18n::CatalogError::ReservedNamespace);
        }
        yard_i18n::register(cx, &self.name, locale, json)
    }
}
