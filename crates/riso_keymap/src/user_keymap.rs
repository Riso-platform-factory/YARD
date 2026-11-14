//! Loads the user's keymap file in Zed's format after the default bindings.

use gpui::{Action, App, Global, KeyBinding, KeyBindingContextPredicate, NoAction};
use serde_json::{Map, Value};
use std::{fs, io, path::Path, rc::Rc};

pub(crate) struct DefaultBindings(
    #[allow(dead_code, reason = "used by keymap_reload while it is in dev")]
    pub(crate)  Vec<KeyBinding>,
);

impl Global for DefaultBindings {}

fn parse_context(
    section: &Map<String, Value>,
) -> Result<(Option<Rc<KeyBindingContextPredicate>>, &str), String> {
    match section.get("context") {
        None => Ok((None, "")),
        Some(Value::String(context)) => KeyBindingContextPredicate::parse(context)
            .map(|predicate| (Some(Rc::new(predicate)), context.as_str()))
            .map_err(|error| error.to_string()),
        Some(_) => Err("the context is not a string".into()),
    }
}

fn build_action(value: &Value, cx: &App) -> Result<Box<dyn Action>, String> {
    match value {
        Value::Null => Ok(Box::new(NoAction)),
        Value::String(name) => cx
            .build_action(name, None)
            .map_err(|error| error.to_string()),
        Value::Array(items) => match items.as_slice() {
            [Value::String(name), input] => cx
                .build_action(name, Some(input.clone()))
                .map_err(|error| error.to_string()),
            _ => Err("expected an action name or `[name, input]`".into()),
        },
        _ => Err("expected `null`, an action name, or `[name, input]`".into()),
    }
}

/// Reads `path` and binds its keys after the defaults. Returns how many bindings were bound.
pub fn load_user_keymap(cx: &mut App, path: &Path) -> usize {
    let _group = tracing::info_span!("keymap-load", path = %path.display()).entered();
    if !cx.has_global::<DefaultBindings>() {
        let defaults = cx.key_bindings().borrow().bindings().cloned().collect();
        cx.set_global(DefaultBindings(defaults));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            tracing::debug!("no keymap file");
            return 0;
        }
        Err(error) => {
            log::warn!("keymap.json: {error}");
            return 0;
        }
    };
    let sections = match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Array(sections)) => sections,
        Ok(_) => {
            log::warn!("keymap.json: the file is not a JSON array");
            return 0;
        }
        Err(error) => {
            log::warn!("keymap.json: {error}");
            return 0;
        }
    };

    let mut bindings = Vec::new();
    let mut logged = Vec::new();
    let mut skipped = 0usize;

    for (index, section) in sections.into_iter().enumerate() {
        let Value::Object(section) = section else {
            skipped += 1;
            log::warn!("keymap.json: section {index}: the section is not a JSON object");
            continue;
        };
        let (context, context_name) = match parse_context(&section) {
            Ok(parsed) => parsed,
            Err(reason) => {
                skipped += 1;
                log::warn!("keymap.json: section {index}: {reason}");
                continue;
            }
        };
        let entries = match section.get("bindings") {
            Some(Value::Object(entries)) => entries,
            Some(_) => {
                skipped += 1;
                log::warn!("keymap.json: section {index}: bindings is not a JSON object");
                continue;
            }
            None => continue,
        };
        for (keys, value) in entries {
            if keys.trim().is_empty() {
                skipped += 1;
                log::warn!("keymap.json: {keys}: empty keystrokes");
                continue;
            }
            let action = match build_action(value, cx) {
                Ok(action) => action,
                Err(reason) => {
                    skipped += 1;
                    log::warn!("keymap.json: {keys}: {reason}");
                    continue;
                }
            };
            let action_name = action.name();
            match KeyBinding::load(keys, action, context.clone(), false, None, cx) {
                Ok(binding) => {
                    logged.push((keys.clone(), action_name, context_name.to_owned()));
                    bindings.push(binding);
                }
                Err(error) => {
                    skipped += 1;
                    log::warn!("keymap.json: {keys}: {error}");
                }
            }
        }
    }

    let count = bindings.len();
    if !bindings.is_empty() {
        cx.bind_keys(bindings);
    }
    for (keys, action, context) in &logged {
        tracing::debug!(
            keys = keys.as_str(),
            action,
            context = context.as_str(),
            "bound"
        );
    }
    tracing::debug!(count, skipped, "user keys bound");
    count
}
