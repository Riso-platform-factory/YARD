use yard_layout::NodeId;

#[test]
fn copies_use_slug_keys_and_cannot_be_parsed_as_nodes() {
    let template = NodeId::parse("name").unwrap();
    assert_eq!(NodeId::copy(&template, "ada").unwrap().as_str(), "name@ada");
    for key in ["Ada", "", "a@b"] {
        assert!(NodeId::copy(&template, key).is_err(), "{key:?}");
    }
    assert!(NodeId::parse("name@ada").is_err());
}
