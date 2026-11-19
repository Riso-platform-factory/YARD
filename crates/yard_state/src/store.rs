//! The versioned JSON format for one extension's named values.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const VERSION: u64 = 1;

pub type StoreValues = BTreeMap<String, Value>;

#[derive(Serialize)]
struct EncodedStore<'a> {
    version: u64,
    values: &'a StoreValues,
}

pub fn encode_store(values: &StoreValues) -> String {
    let _group = tracing::info_span!("store-encode", keys = values.len()).entered();
    let text = serde_json::to_string_pretty(&EncodedStore {
        version: VERSION,
        values,
    })
    .expect("extension values encode as JSON");
    tracing::debug!(bytes = text.len(), "encoded");
    text
}

pub fn decode_store(text: &str) -> Result<StoreValues, String> {
    let _group = tracing::info_span!("store-decode", bytes = text.len()).entered();
    let mut root: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    match root.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(version) => return Err(format!("unsupported version {version}")),
        None => return Err("missing or invalid version".into()),
    }
    let Some(Value::Object(values)) = root.as_object_mut().and_then(|root| root.remove("values"))
    else {
        return Err("expected a map of extension values".into());
    };
    tracing::debug!(keys = values.len(), "decoded");
    Ok(values.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_round_trip() {
        let values = StoreValues::from([
            ("nested".into(), json!({"items": [1, false]})),
            ("empty".into(), Value::Null),
        ]);
        assert_eq!(decode_store(&encode_store(&values)).unwrap(), values);
        let empty = StoreValues::new();
        assert_eq!(decode_store(&encode_store(&empty)).unwrap(), empty);
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(decode_store("{").is_err());
        assert!(decode_store(r#"{"version":1,"values":[]}"#).is_err());
        assert!(decode_store(r#"{"version":1}"#).is_err());
        assert!(decode_store(r#"{"values":{}}"#).is_err());
    }

    #[test]
    fn other_versions_are_refused() {
        assert_eq!(
            decode_store(r#"{"version":2,"values":{}}"#).unwrap_err(),
            "unsupported version 2"
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let values = decode_store(r#"{"version":1,"values":{"count":3},"extra":true}"#).unwrap();
        assert_eq!(values, StoreValues::from([("count".into(), json!(3))]));
    }
}
