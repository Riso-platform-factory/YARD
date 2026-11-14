use super::*;
use serde::Deserialize;
use serde_json::json;

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Section {
    size: u32,
}

impl Settings for Section {
    const KEY: &'static str = "section";
}

fn object(text: &str) -> Map<String, Value> {
    serde_json::from_str(text).unwrap()
}

#[test]
fn replacement_preserves_other_values_and_key_order() {
    let content =
        object(r#"{"z": {"b": 1, "a": 2}, "section": {"extra": true, "size": 3}, "a": 0}"#);
    let changed = change_content::<Section>(content, "size", json!(4))
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_string(&changed).unwrap(),
        r#"{"z":{"b":1,"a":2},"section":{"extra":true,"size":4},"a":0}"#
    );
    assert_eq!(Section::deserialize(&changed["section"]).unwrap().size, 4);
}

#[test]
fn absent_section_is_added_last_with_only_the_written_field() {
    let changed = change_content::<Section>(object(r#"{"z": 1, "a": 2}"#), "size", json!(5))
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_string(&changed).unwrap(),
        r#"{"z":1,"a":2,"section":{"size":5}}"#
    );
}

#[test]
fn output_has_two_space_indentation_and_final_newline() {
    let content = object(r#"{"section": {"size": 4}}"#);
    assert_eq!(
        format_content(&content).unwrap(),
        b"{\n  \"section\": {\n    \"size\": 4\n  }\n}\n"
    );
}

#[test]
fn same_value_is_unchanged_and_bad_sections_are_refused() {
    assert!(
        change_content::<Section>(object(r#"{"section": {"size": 4}}"#), "size", json!(4))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        change_content::<Section>(object(r#"{"section": 4}"#), "size", json!(5)),
        Err(ChangeError::NotObject)
    ));
    assert!(matches!(
        change_content::<Section>(Map::new(), "size", json!("big")),
        Err(ChangeError::Decode(_))
    ));
}

#[test]
fn an_unchanged_invalid_value_is_still_refused_before_the_equality_check() {
    assert!(matches!(
        change_content::<Section>(
            object(r#"{"section": {"size": "big"}}"#),
            "size",
            json!("big")
        ),
        Err(ChangeError::Decode(_))
    ));
}
