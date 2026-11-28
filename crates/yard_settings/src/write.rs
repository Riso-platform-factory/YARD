use crate::{Settings, sections};
use gpui::App;
use serde_json::{Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    Unchanged,
    Refused(WriteRefusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteRefusal {
    UnknownSection,
    UnknownField,
    TypeMismatch,
    NonObjectSection,
    InvalidValue,
    UnreadableFile,
    WriteFailed,
    ConcurrentChange,
}

/// Sets `field` of the section `T` to `value` in the settings file.
pub fn write<T: Settings + serde::Serialize>(cx: &mut App, field: &str, value: Value) {
    write_outcome::<T>(cx, field, value);
}

pub(crate) fn write_outcome<T: Settings + serde::Serialize>(
    cx: &mut App,
    field: &str,
    value: Value,
) -> WriteOutcome {
    use WriteRefusal::*;
    let key = T::KEY;
    let _group = tracing::info_span!("settings-write", section = key, field).entered();
    let result = update_field::<T>(cx, field, |content| {
        change_content::<T>(content, field, value.clone()).map_err(|error| match error {
            ChangeError::NotObject => NonObjectSection,
            ChangeError::Decode(_) => InvalidValue,
        })
    });
    match result {
        Ok(FieldUpdate {
            path,
            content,
            written: true,
        }) => {
            tracing::debug!(path = %path.display(), "written");
            publish(cx, &path, content);
            WriteOutcome::Written
        }
        Ok(FieldUpdate {
            path,
            content,
            written: false,
        }) => {
            tracing::debug!("unchanged");
            publish_if_changed(cx, &path, content);
            WriteOutcome::Unchanged
        }
        Err(reason) => {
            match reason {
                UnknownSection => log::error!("section {key} is not registered"),
                UnknownField => log::error!("section {key} has no field {field}"),
                NonObjectSection => {
                    log::warn!(
                        "the settings file is not written: section {key} is not a JSON object"
                    );
                }
                TypeMismatch => tracing::info!(?reason, "refused"),
                InvalidValue => {
                    log::error!("section {key} cannot take {field}: value does not fit the field")
                }
                UnreadableFile => {
                    log::warn!("the settings file is not written: settings.json could not be read")
                }
                WriteFailed => log::error!("the settings file cannot be written: write failed"),
                ConcurrentChange => {
                    log::warn!("the settings file is not written: it kept changing")
                }
            }
            WriteOutcome::Refused(reason)
        }
    }
}

pub(crate) fn publish(cx: &mut App, path: &Path, content: Map<String, Value>) {
    let _reload = tracing::info_span!("settings-reload", path = %path.display()).entered();
    sections::apply_content(cx, content);
}

type Content = Map<String, Value>;

pub(crate) struct FieldUpdate {
    pub path: PathBuf,
    pub content: Content,
    pub written: bool,
}

pub(crate) fn publish_if_changed(cx: &mut App, path: &Path, content: Content) {
    if !sections::content_matches(cx, &content) {
        publish(cx, path, content);
    }
}

pub(crate) fn update_field<T: Settings + serde::Serialize>(
    cx: &App,
    field: &str,
    mut change: impl FnMut(Content) -> Result<Option<Content>, WriteRefusal>,
) -> Result<FieldUpdate, WriteRefusal> {
    use WriteRefusal::*;
    if !sections::is_registered(cx, T::KEY) {
        return Err(UnknownSection);
    }
    if !sections::is_registered_type::<T>(cx) {
        return Err(TypeMismatch);
    }
    let defaults = serde_json::to_value(T::default()).map_err(|_| InvalidValue)?;
    if !defaults
        .as_object()
        .is_some_and(|defaults| defaults.contains_key(field))
    {
        return Err(UnknownField);
    }
    let path = sections::file_path(cx).ok_or(UnreadableFile)?;
    let mut before = read_bytes(&path)?;
    for _ in 0..=3 {
        let content = parse_snapshot(before.as_deref())?;
        let changed = change(content)?;
        if let Some(content) = changed {
            let bytes = format_content(&content).map_err(|_| WriteFailed)?;
            let mut reread = None;
            let mut read_failed = false;
            let written = yard_paths::write_atomic_if(&path, &bytes, || {
                let current = read_bytes(&path).map_err(|_| {
                    read_failed = true;
                    io::Error::other("settings.json could not be read")
                })?;
                let matches = current == before;
                reread = Some(current);
                Ok(matches)
            })
            .map_err(|_| {
                if read_failed {
                    UnreadableFile
                } else {
                    WriteFailed
                }
            })?;
            if written {
                return Ok(FieldUpdate {
                    path,
                    content,
                    written: true,
                });
            }
            before = reread.expect("the replacement check ran");
        } else {
            let current = read_bytes(&path)?;
            if current == before {
                return Ok(FieldUpdate {
                    path,
                    content: parse_snapshot(current.as_deref())?,
                    written: false,
                });
            }
            before = current;
        }
    }
    Err(ConcurrentChange)
}

fn parse_snapshot(bytes: Option<&[u8]>) -> Result<Content, WriteRefusal> {
    Ok(bytes
        .map(sections::parse_content)
        .transpose()
        .map_err(|_| WriteRefusal::UnreadableFile)?
        .unwrap_or_default())
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, WriteRefusal> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(WriteRefusal::UnreadableFile),
    }
}

#[derive(Debug)]
enum ChangeError {
    NotObject,
    Decode(()),
}

fn change_content<T: Settings>(
    mut content: Content,
    field: &str,
    value: Value,
) -> Result<Option<Content>, ChangeError> {
    let section = content
        .entry(T::KEY)
        .or_insert_with(|| Value::Object(Map::new()));
    let object = section.as_object_mut().ok_or(ChangeError::NotObject)?;
    let unchanged = object.get(field) == Some(&value);
    object.insert(field.to_owned(), value);
    T::deserialize(&*section).map_err(|_| ChangeError::Decode(()))?;
    Ok((!unchanged).then_some(content))
}

fn format_content(content: &Content) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(content)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests;
