use gpui::{App, Global, Subscription};
use riso_paths::dev::{FileCheck, FileState};
use serde_json::{Map, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::Instrument;

/// A section whose defaults provide every setting's initial value.
pub trait Settings: serde::de::DeserializeOwned + Default + 'static {
    const KEY: &'static str;
}

struct Section<T>(T);
impl<T: 'static> Global for Section<T> {}

struct Entry {
    key: &'static str,
    applied: Option<Value>,
    apply: fn(&Map<String, Value>, &mut App) -> Result<(), String>,
}

struct Store {
    entries: Vec<Entry>,
    check: Option<(PathBuf, FileCheck)>,
    watching: bool,
    content: Map<String, Value>,
    loaded: bool,
}
impl Global for Store {}

/// Creates the store once, before any section is registered.
pub fn init(cx: &mut App) {
    let _group = tracing::info_span!("settings-init").entered();
    assert!(!cx.has_global::<Store>(), "riso_settings::init runs once");
    cx.set_global(Store {
        entries: Vec::new(),
        check: None,
        watching: false,
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
    let mut check = FileCheck::new(path.clone());
    let state = check
        .check()
        .expect("the first check always gives the state");
    store.content = match state {
        FileState::Bytes(bytes) => match parse_content(&bytes) {
            Ok(content) => {
                tracing::debug!(sections = content.len(), "loaded");
                content
            }
            Err(reason) => {
                log::warn!("the settings file cannot be read: {reason}");
                Map::new()
            }
        },
        FileState::Missing => {
            tracing::debug!("no settings file");
            Map::new()
        }
        FileState::Unreadable(reason) => {
            log::warn!("the settings file cannot be read: {reason}");
            Map::new()
        }
    };
    store.check = Some((path, check));
}

#[cfg(test)]
fn read_file(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => parse_content(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn parse_content(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    match serde_json::from_slice(bytes) {
        Ok(Value::Object(content)) => Ok(content),
        Ok(_) => Err("the file is not a JSON object".into()),
        Err(error) => Err(error.to_string()),
    }
}

fn changed_sections(
    applied: &[(&'static str, Option<Value>)],
    content: &Map<String, Value>,
) -> Vec<(&'static str, Option<Value>)> {
    applied
        .iter()
        .filter(|(key, old)| content.get(*key) != old.as_ref())
        .map(|(key, _)| (*key, content.get(*key).cloned()))
        .collect()
}

fn apply<T: Settings>(content: &Map<String, Value>, cx: &mut App) -> Result<(), String> {
    let value = decode_section::<T>(content)?.unwrap_or_default();
    cx.set_global(Section(value));
    Ok(())
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
    if store.entries.iter().any(|entry| entry.key == key) {
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
    let applied = store.content.get(key).cloned();
    cx.set_global(Section(value));
    cx.global_mut::<Store>().entries.push(Entry {
        key,
        applied,
        apply: apply::<T>,
    });
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

/// Calls the callback whenever the section is published, until dropped.
pub fn observe<T: Settings>(
    cx: &mut App,
    on_change: impl FnMut(&mut App) + 'static,
) -> Subscription {
    cx.observe_global::<Section<T>>(on_change)
}

fn reload(cx: &mut App, path: &Path, state: FileState) {
    let _group = tracing::info_span!("settings-reload", path = %path.display()).entered();
    let content = match state {
        FileState::Missing => Ok(Map::new()),
        FileState::Bytes(bytes) => parse_content(&bytes),
        FileState::Unreadable(reason) => Err(reason),
    };
    let content = match content {
        Ok(content) => content,
        Err(reason) => {
            log::warn!("the settings file cannot be read: {reason}");
            tracing::debug!(changed = 0, "reloaded");
            return;
        }
    };
    let applied: Vec<_> = cx
        .global::<Store>()
        .entries
        .iter()
        .map(|entry| (entry.key, entry.applied.clone()))
        .collect();
    let pending = changed_sections(&applied, &content);
    let mut changed = 0;
    for (key, value) in pending {
        let index = cx
            .global::<Store>()
            .entries
            .iter()
            .position(|entry| entry.key == key)
            .unwrap();
        let apply = cx.global::<Store>().entries[index].apply;
        let result = apply(&content, cx);
        cx.global_mut::<Store>().entries[index].applied = value;
        match result {
            Ok(()) => {
                changed += 1;
                tracing::debug!(section = key, "changed");
            }
            Err(reason) => log::warn!("section {key} cannot be read: {reason}"),
        }
    }
    cx.global_mut::<Store>().content = content;
    tracing::debug!(changed, "reloaded");
}

/// Checks the loaded settings file once a second for the lifetime of the app.
pub fn watch(cx: &mut App) {
    let span = tracing::info_span!("settings-watch");
    let _group = span.enter();
    let store = cx.global_mut::<Store>();
    if store.watching {
        tracing::debug!("already watching");
        return;
    }
    let Some((path, mut check)) = store.check.take() else {
        tracing::debug!("no settings file to watch");
        return;
    };
    store.watching = true;
    tracing::debug!("started");
    let task_span = span.clone();
    cx.spawn(async move |cx| {
        async move {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if let Some(state) = check.check() {
                    cx.update(|cx| reload(cx, &path, state));
                }
            }
        }
        .instrument(task_span)
        .await
    })
    .detach();
}

#[cfg(test)]
mod tests;
