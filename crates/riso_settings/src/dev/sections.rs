use gpui::{App, Global};

/// A section whose defaults provide every setting's initial value.
pub trait Settings: serde::de::DeserializeOwned + Default + 'static {
    const KEY: &'static str;
}

struct Section<T>(T);
impl<T: 'static> Global for Section<T> {}

struct Store {
    keys: Vec<&'static str>,
}
impl Global for Store {}

/// Creates the store once, before any section is registered.
pub fn init(cx: &mut App) {
    let _group = tracing::info_span!("settings-init").entered();
    assert!(!cx.has_global::<Store>(), "riso_settings::init runs once");
    cx.set_global(Store { keys: Vec::new() });
    tracing::debug!("store created");
}

/// Publishes a section's defaults, keeping the first registration of each key.
pub fn register<T: Settings>(cx: &mut App) {
    let _group = tracing::info_span!("settings-register", section = T::KEY).entered();
    let store = cx
        .try_global::<Store>()
        .expect("riso_settings::init runs before a section is registered");
    let key = T::KEY;
    if store.keys.contains(&key) {
        log::error!("section {key} is already registered");
        return;
    }
    cx.set_global(Section(T::default()));
    cx.global_mut::<Store>().keys.push(key);
    tracing::debug!("registered");
}

/// Returns the published value of a registered section.
pub fn get<T: Settings>(cx: &App) -> &T {
    &cx.try_global::<Section<T>>()
        .unwrap_or_else(|| {
            panic!(
                "section {} is read before it is registered",
                std::any::type_name::<T>()
            )
        })
        .0
}

#[cfg(test)]
mod tests;
