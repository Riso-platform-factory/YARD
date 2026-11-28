use crate::{Settings, WriteOutcome, WriteRefusal, get, observe, sections, write};
use gpui::{App, Global, SharedString, Subscription};
use schemars::{Schema, generate::SchemaSettings, transform::transform_subschemas};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaWrite {
    Written,
    Unchanged,
}

pub struct DescribedSection {
    pub key: &'static str,
    pub title: SharedString,
    pub schema: Value,
    pub value: Value,
    pub defaults: Value,
}

type ChangeListener = Rc<dyn Fn(&mut App)>;

struct Description {
    key: &'static str,
    title: SharedString,
    schema: Schema,
    defaults: Value,
    read: fn(&App) -> Option<Value>,
    write: fn(&mut App, &str, Value) -> WriteOutcome,
    reset: fn(&mut App, &str) -> WriteOutcome,
    observe: fn(&mut App, ChangeListener) -> Subscription,
}

#[derive(Default)]
struct Descriptions(Vec<Description>);

impl Global for Descriptions {}

fn descriptions(cx: &App) -> &[Description] {
    cx.try_global::<Descriptions>()
        .map_or(&[], |descriptions| &descriptions.0)
}

/// Registers and describes a new settings section, keeping the first registration.
pub fn register_described<T: Settings + Serialize + schemars::JsonSchema>(cx: &mut App) {
    let already_registered = sections::is_registered(cx, T::KEY);
    crate::register::<T>(cx);
    if !already_registered {
        describe::<T>(cx);
    }
}

/// Describes a registered settings section for the schema and editor.
pub fn describe<T: Settings + Serialize + schemars::JsonSchema>(cx: &mut App) {
    let _group = tracing::info_span!("settings-describe", section = T::KEY).entered();
    if !sections::is_registered(cx, T::KEY) {
        tracing::info!(reason = "section is not registered", "refused");
        return;
    }
    if !sections::is_registered_type::<T>(cx) {
        tracing::info!(reason = "registered type differs", "refused");
        return;
    }
    if descriptions(cx).iter().any(|section| section.key == T::KEY) {
        return;
    }
    let Ok(defaults) = serde_json::to_value(T::default()) else {
        log::warn!("cannot serialize defaults for section {}", T::KEY);
        return;
    };
    let mut settings = SchemaSettings::draft07();
    settings.inline_subschemas = true;
    let mut schema = settings.into_generator().into_root_schema_for::<T>();
    let generated_title = T::schema_name();
    let title = schema
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| *title != generated_title)
        .or_else(|| {
            schema
                .get("description")
                .and_then(Value::as_str)
                .and_then(|help| help.lines().next())
        })
        .unwrap_or(T::KEY);
    let title = SharedString::from(title.to_owned());
    schema.insert("title".into(), json!(title.as_ref()));
    schema.remove("$schema");
    cx.default_global::<Descriptions>().0.push(Description {
        key: T::KEY,
        title,
        schema,
        defaults,
        read: |cx| serde_json::to_value(get::<T>(cx)).ok(),
        write: write::write_outcome::<T>,
        reset: reset_outcome::<T>,
        observe: |cx, callback| observe::<T>(cx, move |cx| callback(cx)),
    });
    tracing::debug!("described");
}

/// Builds the Draft 07 schema for the whole settings file.
pub fn json_schema(cx: &App) -> Value {
    let mut properties = Map::new();
    properties.insert("$schema".into(), json!({ "type": "string" }));
    // TODO(settings-ui): Describe the extensions section with dynamic keys. Generate a boolean description for each extension key.
    for section in descriptions(cx) {
        let mut schema = section.schema.clone();
        let prefix = format!("#/properties/{}", fragment_token(section.key));
        rebase_references(&mut schema, &prefix);
        properties.insert(section.key.into(), schema.to_value());
    }
    let mut root = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": properties,
        "additionalProperties": true,
    });
    root.sort_all_objects();
    root
}

fn fragment_token(key: &str) -> String {
    let pointer = key.replace('~', "~0").replace('/', "~1");
    let mut token = String::new();
    for byte in pointer.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            token.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(token, "%{byte:02X}");
        }
    }
    token
}

fn rebase_references(schema: &mut Schema, prefix: &str) {
    // An explicit resource ID defines the base for its own references.
    if schema.get("$id").is_some() {
        return;
    }
    if let Some(fragment) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix('#'))
        && (fragment.is_empty() || fragment.starts_with('/'))
    {
        schema.insert("$ref".into(), json!(format!("{prefix}{fragment}")));
    }
    transform_subschemas(
        &mut |child: &mut Schema| rebase_references(child, prefix),
        schema,
    );
}

/// Writes the schema atomically when its bytes differ from the existing file.
pub fn write_schema_file(cx: &App, path: &Path) -> io::Result<SchemaWrite> {
    // TODO(settings-ui): Add $schema to settings.json automatically. Decide whether YARD may write the reference into the user's file.
    let _group =
        tracing::info_span!("settings-schema-write", sections = descriptions(cx).len()).entered();
    let prepared = (|| {
        let mut bytes = serde_json::to_vec_pretty(&json_schema(cx))?;
        bytes.push(b'\n');
        match fs::read(path) {
            Ok(current) if current == bytes => return Ok(None),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(Some(bytes))
    })()
    .inspect_err(|error| {
        log::warn!("settings schema could not be written: {error}");
    });
    prepared
        .and_then(|bytes| {
            if let Some(bytes) = bytes {
                // The atomic writer records its own diagnostic warning on failure.
                yard_paths::write_atomic(path, &bytes)?;
                tracing::debug!("written");
                Ok(SchemaWrite::Written)
            } else {
                tracing::debug!("unchanged");
                Ok(SchemaWrite::Unchanged)
            }
        })
        .inspect_err(|error| tracing::warn!(%error, "settings schema could not be written"))
}

/// Removes a field override and republishes the effective settings.
pub fn reset<T: Settings + Serialize>(cx: &mut App, field: &str) {
    reset_outcome::<T>(cx, field);
}

fn reset_outcome<T: Settings + Serialize>(cx: &mut App, field: &str) -> WriteOutcome {
    use WriteRefusal::*;
    let section = T::KEY;
    let _group = tracing::info_span!("settings-reset", section, field).entered();
    let result = write::update_field::<T>(cx, field, |mut content| {
        let Some(value) = content.get_mut(section) else {
            return Ok(None);
        };
        let fields = value.as_object_mut().ok_or(NonObjectSection)?;
        if fields.shift_remove(field).is_none() {
            return Ok(None);
        }
        if fields.is_empty() {
            content.shift_remove(section);
        } else {
            T::deserialize(&*value).map_err(|_| InvalidValue)?;
        }
        Ok(Some(content))
    });
    match result {
        Ok(write::FieldUpdate {
            path,
            content,
            written: true,
        }) => {
            tracing::debug!("reset");
            write::publish(cx, &path, content);
            WriteOutcome::Written
        }
        Ok(write::FieldUpdate {
            path,
            content,
            written: false,
        }) => {
            tracing::debug!("unchanged");
            write::publish_if_changed(cx, &path, content);
            WriteOutcome::Unchanged
        }
        Err(reason) => {
            match reason {
                UnknownSection | UnknownField | TypeMismatch => tracing::info!(?reason, "refused"),
                _ => {
                    tracing::warn!(?reason, "settings could not be reset");
                    log::warn!("settings could not be reset for {section}.{field}: {reason:?}");
                }
            }
            WriteOutcome::Refused(reason)
        }
    }
}

/// Returns the published values and schemas for the described sections.
pub fn described_sections(cx: &App) -> Vec<DescribedSection> {
    descriptions(cx)
        .iter()
        .filter_map(|section| {
            Some(DescribedSection {
                key: section.key,
                title: section.title.clone(),
                schema: section.schema.clone().to_value(),
                value: (section.read)(cx)?,
                defaults: section.defaults.clone(),
            })
        })
        .collect()
}

/// Observes publication of settings in the currently described sections.
pub fn observe_described_sections(
    cx: &mut App,
    on_change: impl Fn(&mut App) + 'static,
) -> Vec<Subscription> {
    let listeners: Vec<_> = descriptions(cx)
        .iter()
        .map(|section| section.observe)
        .collect();
    let on_change: ChangeListener = Rc::new(on_change);
    listeners
        .into_iter()
        .map(|observe| observe(cx, on_change.clone()))
        .collect()
}

/// Returns the path of the settings file loaded by the application.
pub fn settings_path(cx: &App) -> Option<PathBuf> {
    sections::file_path(cx)
}

// TODO(settings-ui): Preserve JSONC comments and trailing commas when reading and writing settings.json. Adopt the MIT-licensed jsonc-parser.
pub fn write_value(cx: &mut App, section: &str, field: &str, value: Value) -> WriteOutcome {
    if let Some(write) = descriptions(cx)
        .iter()
        .find(|entry| entry.key == section)
        .map(|entry| entry.write)
    {
        write(cx, field, value)
    } else {
        let _group = tracing::info_span!("settings-write", section, field).entered();
        tracing::info!(reason = "section is not described", "refused");
        WriteOutcome::Refused(WriteRefusal::UnknownSection)
    }
}

pub fn reset_value(cx: &mut App, section: &str, field: &str) -> WriteOutcome {
    if let Some(reset) = descriptions(cx)
        .iter()
        .find(|entry| entry.key == section)
        .map(|entry| entry.reset)
    {
        reset(cx, field)
    } else {
        let _group = tracing::info_span!("settings-reset", section, field).entered();
        tracing::info!(reason = "section is not described", "refused");
        WriteOutcome::Refused(WriteRefusal::UnknownSection)
    }
}
