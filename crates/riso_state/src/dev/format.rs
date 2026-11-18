//! JSON encoding and decoding of the state of nodes in session order.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const VERSION: u64 = 1;

/// One node's state and the ID of the component that gave it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedNode {
    pub component: String,
    pub state: Value,
}

/// The saved nodes in one window, keyed by node ID.
pub type WindowStates = BTreeMap<String, SavedNode>;

#[derive(Serialize)]
struct EncodedFile<'a> {
    version: u64,
    windows: &'a [WindowStates],
}

#[derive(Deserialize)]
struct SavedFile {
    windows: Vec<WindowStates>,
}

/// Encodes the windows as version 1 JSON.
pub fn encode(windows: &[WindowStates]) -> String {
    let _group = tracing::info_span!("state-encode", windows = windows.len()).entered();
    let text = serde_json::to_string_pretty(&EncodedFile {
        version: VERSION,
        windows,
    })
    .expect("node state encodes as JSON");
    tracing::debug!(bytes = text.len(), "encoded");
    text
}

/// Decodes version 1 JSON, allowing unknown fields.
pub fn decode(text: &str) -> Result<Vec<WindowStates>, String> {
    let _group = tracing::info_span!("state-decode", bytes = text.len()).entered();
    let root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match root.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(version) => return Err(format!("unsupported version {version}")),
        None => return Err("missing or invalid version".into()),
    }
    let windows_are_maps = root
        .get("windows")
        .and_then(Value::as_array)
        .is_some_and(|windows| {
            windows.iter().all(|window| {
                window
                    .as_object()
                    .is_some_and(|nodes| nodes.values().all(Value::is_object))
            })
        });
    if !windows_are_maps {
        return Err("expected windows containing maps of node states".into());
    }
    let saved: SavedFile = serde_json::from_value(root).map_err(|error| error.to_string())?;
    tracing::debug!(windows = saved.windows.len(), "decoded");
    Ok(saved.windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_round_trip_in_order() {
        let windows = vec![
            BTreeMap::from([(
                "card".to_owned(),
                SavedNode {
                    component: "tour.card".into(),
                    state: serde_json::json!({"count": 3}),
                },
            )]),
            WindowStates::new(),
        ];
        assert_eq!(decode(&encode(&windows)).unwrap(), windows);
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(decode("{").is_err());
        assert!(decode(r#"{"version":1,"windows":null}"#).is_err());
        assert!(decode(r#"{"version":1,"windows":[{"a":["tour.card", null]}]}"#).is_err());
    }

    #[test]
    fn other_versions_are_refused() {
        assert_eq!(
            decode(r#"{"version":2,"windows":[]}"#).unwrap_err(),
            "unsupported version 2"
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let windows = decode(
            r#"{"version":1,"extra":true,"windows":[{"a":{"component":"tour.card","state":null,"extra":1}}]}"#,
        )
        .unwrap();
        assert_eq!(windows[0]["a"].component, "tour.card");
        assert_eq!(windows[0]["a"].state, Value::Null);
    }
}
