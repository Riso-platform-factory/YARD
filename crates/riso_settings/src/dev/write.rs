use crate::{Settings, sections};
use gpui::App;
use serde_json::{Map, Value};
use std::{fs, io, path::Path};

/// Sets `field` of the section `T` to `value` in the settings file.
pub fn write<T: Settings + serde::Serialize>(cx: &mut App, field: &str, value: Value) {
    let key = T::KEY;
    let _group = tracing::info_span!("settings-write", section = key, field).entered();
    if !sections::is_registered(cx, key) {
        log::error!("section {key} is not registered");
        return;
    }
    let defaults = match serde_json::to_value(T::default()) {
        Ok(defaults) => defaults,
        Err(reason) => {
            log::error!("section {key} cannot take {field}: {reason}");
            return;
        }
    };
    if !defaults
        .as_object()
        .is_some_and(|defaults| defaults.contains_key(field))
    {
        log::error!("section {key} has no field {field}");
        return;
    }
    let Some(path) = sections::file_path(cx) else {
        log::warn!("the settings file is not written: no settings file was loaded");
        return;
    };
    let content = match read_content(&path) {
        Ok(content) => content,
        Err(reason) => {
            log::warn!("the settings file is not written: {reason}");
            return;
        }
    };
    let content = match change_content::<T>(content, field, value) {
        Ok(Some(content)) => content,
        Ok(None) => {
            tracing::debug!("unchanged");
            return;
        }
        Err(ChangeError::NotObject) => {
            log::warn!("the settings file is not written: the section is not a JSON object");
            return;
        }
        Err(ChangeError::Decode(reason)) => {
            log::error!("section {key} cannot take {field}: {reason}");
            return;
        }
    };
    let bytes = match format_content(&content) {
        Ok(bytes) => bytes,
        Err(reason) => {
            log::error!("the settings file cannot be written: {reason}");
            return;
        }
    };
    if let Err(reason) = riso_paths::write_atomic(&path, &bytes) {
        log::error!("the settings file cannot be written: {reason}");
        return;
    }
    tracing::debug!(path = %path.display(), "written");
    let _reload = tracing::info_span!("settings-reload", path = %path.display()).entered();
    sections::apply_content(cx, content);
}

fn read_content(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error.to_string()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(content)) => Ok(content),
        Ok(_) => Err("the file is not a JSON object".into()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug)]
enum ChangeError {
    NotObject,
    Decode(String),
}

fn change_content<T: Settings>(
    mut content: Map<String, Value>,
    field: &str,
    value: Value,
) -> Result<Option<Map<String, Value>>, ChangeError> {
    let section = content
        .entry(T::KEY)
        .or_insert_with(|| Value::Object(Map::new()));
    let object = section.as_object_mut().ok_or(ChangeError::NotObject)?;
    let unchanged = object.get(field) == Some(&value);
    object.insert(field.to_owned(), value);
    T::deserialize(&*section).map_err(|error| ChangeError::Decode(error.to_string()))?;
    Ok((!unchanged).then_some(content))
}

fn format_content(content: &Map<String, Value>) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(content)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests;
