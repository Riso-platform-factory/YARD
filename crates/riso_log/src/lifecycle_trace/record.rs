use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A record in the current run's lifecycle JSON Lines file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Record {
    RunStart {
        t: u64,
        name: String,
        version: String,
        identifier: String,
        os: String,
    },
    GroupStart {
        t: u64,
        id: u64,
        parent: Option<u64>,
        name: String,
        fields: Map<String, Value>,
        thread: String,
    },
    Line {
        t: u64,
        group: u64,
        level: String,
        target: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        known: Option<String>,
        fields: Map<String, Value>,
    },
    GroupEnd {
        t: u64,
        id: u64,
        outcome: Outcome,
    },
    RunEnd {
        t: u64,
    },
}

/// Whether a group or any descendant logged an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Success,
    KnownFailure,
    Failure,
    /// The run ended while the group was open.
    Interrupted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips() {
        let records = [
            Record::RunStart {
                t: 0,
                name: "sample".into(),
                version: "0.1".into(),
                identifier: "com.example.sample".into(),
                os: "macos".into(),
            },
            Record::GroupStart {
                t: 1,
                id: 1,
                parent: None,
                name: "app-startup".into(),
                fields: Map::new(),
                thread: "main".into(),
            },
            Record::Line {
                t: 2,
                group: 1,
                level: "INFO".into(),
                target: "sample".into(),
                message: "hello".into(),
                known: None,
                fields: serde_json::from_str(r#"{"count":1,"ok":true,"name":"sample"}"#).unwrap(),
            },
            Record::GroupEnd {
                t: 3,
                id: 1,
                outcome: Outcome::Success,
            },
            Record::GroupEnd {
                t: 4,
                id: 2,
                outcome: Outcome::Failure,
            },
            Record::Line {
                t: 5,
                group: 2,
                level: "ERROR".into(),
                target: "test".into(),
                message: "known error".into(),
                known: Some("test-known".into()),
                fields: Map::new(),
            },
            Record::GroupEnd {
                t: 6,
                id: 2,
                outcome: Outcome::KnownFailure,
            },
            Record::RunEnd { t: 7 },
        ];
        for record in records {
            let json = serde_json::to_string(&record).unwrap();
            assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), record);
            if matches!(record, Record::Line { known: None, .. }) {
                assert!(
                    serde_json::from_str::<Value>(&json)
                        .unwrap()
                        .get("known")
                        .is_none()
                );
            }
            if matches!(
                record,
                Record::GroupEnd {
                    outcome: Outcome::KnownFailure,
                    ..
                }
            ) {
                assert_eq!(
                    serde_json::from_str::<Value>(&json).unwrap()["outcome"],
                    "known-failure"
                );
            }
        }
    }
}
