use gpui::{App, Global};
use serde_json::{Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// A section whose defaults provide every setting's initial value.
pub trait Settings: serde::de::DeserializeOwned + Default + 'static {
    const KEY: &'static str;
}

struct Section<T>(T);
impl<T: 'static> Global for Section<T> {}

struct Store {
    keys: Vec<&'static str>,
    content: Map<String, Value>,
    loaded: bool,
}
impl Global for Store {}

/// Creates the store once, before any section is registered.
pub fn init(cx: &mut App) {
    let _group = tracing::info_span!("settings-init").entered();
    assert!(!cx.has_global::<Store>(), "riso_settings::init runs once");
    cx.set_global(Store {
        keys: Vec::new(),
        content: Map::new(),
        loaded: false,
    });
    tracing::debug!("store created");
}

/// Reads the user's settings file once, before sections are registered.
pub fn load(cx: &mut App, path: PathBuf) {
    let _group = tracing::info_span!("settings-load", path = %path.display()).entered();
    let store = cx.global_mut::<Store>();
    assert!(!store.loaded, "riso_settings::load runs once");
    store.loaded = true;
    store.content = match read_file(&path) {
        Ok(Some(content)) => {
            tracing::debug!(sections = content.len(), "loaded");
            content
        }
        Ok(None) => {
            tracing::debug!("no settings file");
            Map::new()
        }
        Err(reason) => {
            log::warn!("the settings file cannot be read: {reason}");
            Map::new()
        }
    };
}

fn read_file(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(Value::Object(content)) => Ok(Some(content)),
            Ok(_) => Err("the file is not a JSON object".into()),
            Err(error) => Err(error.to_string()),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn decode_section<T: Settings>(content: &Map<String, Value>) -> Result<Option<T>, String> {
    match content.get(T::KEY) {
        Some(value @ Value::Object(_)) => T::deserialize(value)
            .map(Some)
            .map_err(|error| error.to_string()),
        Some(_) => Err("the section is not a JSON object".into()),
        None => Ok(None),
    }
}

/// Publishes a section's file value or defaults, keeping its first registration.
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
    let (value, from) = match decode_section::<T>(&store.content) {
        Ok(Some(value)) => (value, "file"),
        Ok(None) => (T::default(), "defaults"),
        Err(reason) => {
            log::warn!("section {key} cannot be read: {reason}");
            (T::default(), "defaults")
        }
    };
    cx.set_global(Section(value));
    cx.global_mut::<Store>().keys.push(key);
    tracing::debug!(from, "registered");
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
