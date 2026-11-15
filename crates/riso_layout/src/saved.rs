//! JSON encoding and decoding of the saved user layer.

use crate::{ComponentId, IdError, Node, NodeId, Operation, Props, SlotName};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

const FORMAT_VERSION: u32 = 1;

/// A saved user layer that could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Syntax or shape error in the JSON document.
    InvalidJson(String),
    /// Missing or unsupported format version.
    Version(Option<i64>),
    /// An operation whose `"op"` tag is not one of the known kinds.
    UnknownOp(String),
    /// An ID, extension name, or slot name that breaks the slug rules.
    InvalidId {
        field: &'static str,
        value: String,
        error: IdError,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(message) => write!(f, "invalid JSON: {message}"),
            Self::Version(None) => f.write_str("missing version"),
            Self::Version(Some(version)) => write!(f, "unsupported version {version}"),
            Self::UnknownOp(op) => write!(f, "unknown operation {op:?}"),
            Self::InvalidId {
                field,
                value,
                error,
            } => write!(f, "invalid {field} {value:?}: {error}"),
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidId { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct EncodedFile<'a> {
    version: u32,
    operations: Vec<EncodedOp<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum EncodedOp<'a> {
    Add {
        node: &'a str,
        component: &'a str,
        extension: &'a str,
        parent: &'a str,
        slot: &'a str,
        #[serde(skip_serializing_if = "Props::is_empty")]
        props: &'a Props,
    },
    Remove {
        node: &'a str,
    },
    Move {
        node: &'a str,
        parent: &'a str,
        slot: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    Hide {
        node: &'a str,
    },
    Show {
        node: &'a str,
    },
    Props {
        node: &'a str,
        props: &'a Props,
    },
}

#[derive(Deserialize)]
struct VersionProbe {
    version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedFile {
    #[serde(rename = "version")]
    _version: u32,
    operations: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct OpTag {
    op: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedAdd {
    #[serde(rename = "op")]
    _op: String,
    node: String,
    component: String,
    extension: String,
    parent: String,
    slot: String,
    #[serde(default)]
    props: Props,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedNodeOp {
    #[serde(rename = "op")]
    _op: String,
    node: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedProps {
    #[serde(rename = "op")]
    _op: String,
    node: String,
    props: Props,
}

fn last_index() -> usize {
    usize::MAX
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedMove {
    #[serde(rename = "op")]
    _op: String,
    node: String,
    parent: String,
    slot: String,
    #[serde(default = "last_index")]
    index: usize,
}

/// Encodes `ops` as pretty-printed version 1 JSON.
pub fn encode(ops: &[Operation]) -> String {
    let operations = ops
        .iter()
        .map(|op| match op {
            Operation::Add { node, parent, slot } => EncodedOp::Add {
                node: node.id.as_str(),
                component: node.component.as_str(),
                extension: node.extension.as_str(),
                parent: parent.as_str(),
                slot: slot.as_str(),
                props: &node.props,
            },
            Operation::Remove { node } => EncodedOp::Remove {
                node: node.as_str(),
            },
            Operation::Move {
                node,
                parent,
                slot,
                index,
            } => EncodedOp::Move {
                node: node.as_str(),
                parent: parent.as_str(),
                slot: slot.as_str(),
                index: (*index != usize::MAX).then_some(*index),
            },
            Operation::SetHidden { node, hidden: true } => EncodedOp::Hide {
                node: node.as_str(),
            },
            Operation::SetHidden {
                node,
                hidden: false,
            } => EncodedOp::Show {
                node: node.as_str(),
            },
            Operation::SetProps { node, props } => EncodedOp::Props {
                node: node.as_str(),
                props,
            },
        })
        .collect();
    serde_json::to_string_pretty(&EncodedFile {
        version: FORMAT_VERSION,
        operations,
    })
    .expect("operations serialize to JSON")
}

/// Decodes a saved user layer, checking the version, operation kinds, and slug rules.
pub fn decode(text: &str) -> Result<Vec<Operation>, DecodeError> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
    if !root.is_object() {
        return Err(DecodeError::InvalidJson("expected a JSON object".into()));
    }
    let probe: VersionProbe = serde_json::from_value(root.clone())
        .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
    if probe.version != Some(i64::from(FORMAT_VERSION)) {
        return Err(DecodeError::Version(probe.version));
    }
    let file: SavedFile = serde_json::from_value(root)
        .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
    file.operations.into_iter().map(decode_operation).collect()
}

fn decode_operation(entry: serde_json::Value) -> Result<Operation, DecodeError> {
    if !entry.is_object() {
        return Err(DecodeError::InvalidJson(
            "expected an operation object".into(),
        ));
    }
    let tag: OpTag = serde_json::from_value(entry.clone())
        .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
    match tag.op.as_str() {
        "add" => {
            let saved: SavedAdd = serde_json::from_value(entry)
                .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
            let id = parse_node_id("node", saved.node)?;
            let component = parse_component_id("component", saved.component)?;
            let extension = parse_node_id("extension", saved.extension)?;
            let parent = parse_node_id("parent", saved.parent)?;
            let slot = parse_slot_name("slot", saved.slot)?;
            Ok(Operation::Add {
                node: Node {
                    id,
                    component,
                    extension: extension.as_str().to_owned(),
                    props: saved.props,
                },
                parent,
                slot,
            })
        }
        "remove" => {
            let saved: SavedNodeOp = serde_json::from_value(entry)
                .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
            Ok(Operation::Remove {
                node: parse_node_id("node", saved.node)?,
            })
        }
        "move" => {
            let saved: SavedMove = serde_json::from_value(entry)
                .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
            Ok(Operation::Move {
                node: parse_node_id("node", saved.node)?,
                parent: parse_node_id("parent", saved.parent)?,
                slot: parse_slot_name("slot", saved.slot)?,
                index: saved.index,
            })
        }
        "hide" | "show" => {
            let hidden = tag.op == "hide";
            let saved: SavedNodeOp = serde_json::from_value(entry)
                .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
            Ok(Operation::SetHidden {
                node: parse_node_id("node", saved.node)?,
                hidden,
            })
        }
        "props" => {
            let saved: SavedProps = serde_json::from_value(entry)
                .map_err(|error| DecodeError::InvalidJson(error.to_string()))?;
            Ok(Operation::SetProps {
                node: parse_node_id("node", saved.node)?,
                props: saved.props,
            })
        }
        _ => Err(DecodeError::UnknownOp(tag.op)),
    }
}

fn parse_node_id(field: &'static str, value: String) -> Result<NodeId, DecodeError> {
    NodeId::parse(&value).map_err(|error| DecodeError::InvalidId {
        field,
        value,
        error,
    })
}

fn parse_component_id(field: &'static str, value: String) -> Result<ComponentId, DecodeError> {
    ComponentId::parse(&value).map_err(|error| DecodeError::InvalidId {
        field,
        value,
        error,
    })
}

fn parse_slot_name(field: &'static str, value: String) -> Result<SlotName, DecodeError> {
    SlotName::parse(&value).map_err(|error| DecodeError::InvalidId {
        field,
        value,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_operations() -> Vec<Operation> {
        vec![
            Operation::Add {
                node: Node {
                    id: NodeId::parse("extra").unwrap(),
                    component: ComponentId::parse("layout_tour.card").unwrap(),
                    extension: "layout_tour".into(),
                    props: Props::new(),
                },
                parent: NodeId::parse("root").unwrap(),
                slot: SlotName::parse("children").unwrap(),
            },
            Operation::Remove {
                node: NodeId::parse("old").unwrap(),
            },
            Operation::Move {
                node: NodeId::parse("a").unwrap(),
                parent: NodeId::parse("right").unwrap(),
                slot: SlotName::parse("children").unwrap(),
                index: 2,
            },
            Operation::Move {
                node: NodeId::parse("b").unwrap(),
                parent: NodeId::parse("right").unwrap(),
                slot: SlotName::parse("children").unwrap(),
                index: usize::MAX,
            },
            Operation::SetHidden {
                node: NodeId::parse("d").unwrap(),
                hidden: true,
            },
            Operation::SetHidden {
                node: NodeId::parse("d").unwrap(),
                hidden: false,
            },
        ]
    }

    #[test]
    fn every_operation_kind_round_trips() {
        assert_eq!(decode(&encode(&[])), Ok(Vec::new()));
        let ops = sample_operations();
        let text = encode(&ops);
        assert_eq!(decode(&text), Ok(ops));
    }

    #[test]
    fn move_to_the_end_omits_index_and_round_trips() {
        let ops = vec![Operation::Move {
            node: NodeId::parse("a").unwrap(),
            parent: NodeId::parse("right").unwrap(),
            slot: SlotName::parse("children").unwrap(),
            index: usize::MAX,
        }];
        let text = encode(&ops);
        assert!(!text.contains("index"), "{text}");
        assert_eq!(decode(&text), Ok(ops));
    }

    #[test]
    fn pinned_json_matches_the_version_one_format() {
        let pinned = r#"{
  "version": 1,
  "operations": [
    {
      "op": "add",
      "node": "extra",
      "component": "layout_tour.card",
      "extension": "layout_tour",
      "parent": "root",
      "slot": "children"
    },
    {
      "op": "remove",
      "node": "old"
    },
    {
      "op": "move",
      "node": "a",
      "parent": "right",
      "slot": "children",
      "index": 2
    },
    {
      "op": "move",
      "node": "b",
      "parent": "right",
      "slot": "children"
    },
    {
      "op": "hide",
      "node": "d"
    },
    {
      "op": "show",
      "node": "d"
    }
  ]
}"#;
        let ops = sample_operations();
        assert_eq!(encode(&ops), pinned);
        assert_eq!(decode(pinned), Ok(ops));
    }

    #[test]
    fn decode_refuses_invalid_json_versions_unknown_ops_and_bad_slugs() {
        for bad_json in [
            "not json",
            "[]",
            r#"{"version": 1}"#,
            r#"{"version": "1", "operations": []}"#,
            r#"{"version": 1, "operations": "nope"}"#,
            r#"{"version": 1, "operations": [], "extra": true}"#,
            r#"{"version": 1, "operations": [1]}"#,
            r#"{"version": 1, "operations": [{}]}"#,
            r#"{"version": 1, "operations": [{"op": "hide"}]}"#,
            r#"{"version": 1, "operations": [{"op": "hide", "node": "a", "extra": 1}]}"#,
            r#"{"version": 1, "operations": [{"op": "move", "node": "a", "parent": "b", "slot": "c", "index": -1}]}"#,
            r#"{"version": 1, "operations": [{"op": "move", "node": "a", "parent": "b", "slot": "c", "index": null}]}"#,
        ] {
            let error = decode(bad_json).unwrap_err();
            assert!(
                matches!(error, DecodeError::InvalidJson(_)),
                "{bad_json} -> {error:?}"
            );
            assert!(error.to_string().starts_with("invalid JSON: "));
            assert!(error.source().is_none());
        }

        assert_eq!(
            decode(r#"{"operations": []}"#),
            Err(DecodeError::Version(None))
        );
        assert_eq!(DecodeError::Version(None).to_string(), "missing version");

        for version in [0, 2, -1] {
            let text = format!(r#"{{"version": {version}, "operations": []}}"#);
            let error = decode(&text).unwrap_err();
            assert_eq!(error, DecodeError::Version(Some(version)));
            assert_eq!(error.to_string(), format!("unsupported version {version}"));
            assert!(error.source().is_none());
        }

        let unknown =
            decode(r#"{"version": 1, "operations": [{"op": "zoom", "node": "a"}]}"#).unwrap_err();
        assert_eq!(unknown, DecodeError::UnknownOp("zoom".into()));
        assert_eq!(unknown.to_string(), r#"unknown operation "zoom""#);
        assert!(unknown.source().is_none());

        for (expected_field, op_json) in [
            (
                "node",
                r#"{"op": "add", "node": "Bad", "component": "ext.card", "extension": "ext", "parent": "root", "slot": "body"}"#,
            ),
            (
                "component",
                r#"{"op": "add", "node": "a", "component": "noperiod", "extension": "ext", "parent": "root", "slot": "body"}"#,
            ),
            (
                "extension",
                r#"{"op": "add", "node": "a", "component": "ext.card", "extension": "Bad Ext", "parent": "root", "slot": "body"}"#,
            ),
            (
                "parent",
                r#"{"op": "add", "node": "a", "component": "ext.card", "extension": "ext", "parent": "1bad", "slot": "body"}"#,
            ),
            (
                "slot",
                r#"{"op": "add", "node": "a", "component": "ext.card", "extension": "ext", "parent": "root", "slot": ""}"#,
            ),
            ("node", r#"{"op": "remove", "node": "Bad"}"#),
            (
                "node",
                r#"{"op": "move", "node": "Bad", "parent": "root", "slot": "body", "index": 0}"#,
            ),
            (
                "parent",
                r#"{"op": "move", "node": "a", "parent": "Bad", "slot": "body", "index": 0}"#,
            ),
            (
                "slot",
                r#"{"op": "move", "node": "a", "parent": "root", "slot": "Bad.Slot", "index": 0}"#,
            ),
            ("node", r#"{"op": "hide", "node": "-bad"}"#),
            ("node", r#"{"op": "show", "node": "bad/id"}"#),
        ] {
            let text = format!(r#"{{"version": 1, "operations": [{op_json}]}}"#);
            let error = decode(&text).unwrap_err();
            match &error {
                DecodeError::InvalidId { field, .. } => assert_eq!(*field, expected_field),
                other => panic!("expected InvalidId for {expected_field}, got {other:?}"),
            }
            assert!(error.to_string().contains(expected_field));
            assert!(error.source().is_some());
        }
    }
}
