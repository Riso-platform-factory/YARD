use serde_json::json;
use yard_layout::{
    Applied, ComponentId, Layers, Node, NodeId, Operation, Position, Props, SlotName, Tree, decode,
    encode,
};

fn id(value: &str) -> NodeId {
    NodeId::parse(value).unwrap()
}

fn slot(value: &str) -> SlotName {
    SlotName::parse(value).unwrap()
}

fn node(name: &str) -> Node {
    Node {
        id: id(name),
        component: ComponentId::parse("tour.card").unwrap(),
        extension: "tour".into(),
        props: Props::new(),
    }
}

fn add(name: &str, parent: &str, slot_name: &str, at: Position) -> Operation {
    Operation::Add {
        node: node(name),
        parent: id(parent),
        slot: slot(slot_name),
        at,
    }
}

fn move_to(name: &str, parent: &str, slot_name: &str, at: Position) -> Operation {
    Operation::Move {
        node: id(name),
        parent: id(parent),
        slot: slot(slot_name),
        at,
    }
}

fn children_of(tree: &Tree, parent: &str, name: &str) -> Vec<String> {
    tree.slots(&id(parent))
        .find(|(slot, _)| slot.as_str() == name)
        .map(|(_, nodes)| nodes.iter().map(|node| node.id.to_string()).collect())
        .unwrap_or_default()
}

type Snapshot = Vec<(String, Props, bool, Vec<(String, Vec<String>)>)>;

fn snapshot(tree: &Tree) -> Snapshot {
    fn visit(tree: &Tree, node: &Node, out: &mut Snapshot) {
        out.push((
            node.id.to_string(),
            node.props.clone(),
            tree.is_hidden(&node.id),
            tree.slots(&node.id)
                .map(|(slot, children)| {
                    (
                        slot.to_string(),
                        children.iter().map(|child| child.id.to_string()).collect(),
                    )
                })
                .collect(),
        ));
        for (_, children) in tree.slots(&node.id) {
            for child in children {
                visit(tree, child, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(root) = tree.root() {
        visit(tree, root, &mut out);
    }
    out
}

fn tree() -> Tree {
    let mut tree = Tree::default();
    tree.place_root(node("root")).unwrap();
    for name in ["a", "b", "c"] {
        tree.place(node(name), &id("root"), slot("children"))
            .unwrap();
    }
    tree
}

#[test]
fn place_at_puts_the_node_where_the_position_says() {
    let mut tree = tree();
    // First puts the node at the front of the slot.
    tree.place_at(
        node("first"),
        &id("root"),
        slot("children"),
        Position::First,
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["first", "a", "b", "c"]
    );
    // Before(x) puts it right before x, After(x) right after x.
    tree.place_at(
        node("pre"),
        &id("root"),
        slot("children"),
        Position::Before(id("b")),
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["first", "a", "pre", "b", "c"]
    );
    tree.place_at(
        node("post"),
        &id("root"),
        slot("children"),
        Position::After(id("b")),
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["first", "a", "pre", "b", "post", "c"]
    );
    // Index(n) puts it at that index; past the end means last.
    tree.place_at(
        node("one"),
        &id("root"),
        slot("children"),
        Position::Index(1),
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["first", "one", "a", "pre", "b", "post", "c"]
    );
    tree.place_at(
        node("over"),
        &id("root"),
        slot("children"),
        Position::Index(99),
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["first", "one", "a", "pre", "b", "post", "c", "over"]
    );
    // Last puts it at the end, and place() still means last.
    tree.place_at(node("last"), &id("root"), slot("children"), Position::Last)
        .unwrap();
    tree.place(node("plain"), &id("root"), slot("children"))
        .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        [
            "first", "one", "a", "pre", "b", "post", "c", "over", "last", "plain"
        ]
    );
}

#[test]
fn add_puts_the_node_where_the_position_says() {
    let mut tree = tree();
    let cases: [(&str, Position, Vec<&str>); 6] = [
        ("first", Position::First, vec!["first", "a", "b", "c"]),
        (
            "pre",
            Position::Before(id("c")),
            vec!["first", "a", "b", "pre", "c"],
        ),
        (
            "post",
            Position::After(id("c")),
            vec!["first", "a", "b", "pre", "c", "post"],
        ),
        (
            "one",
            Position::Index(1),
            vec!["first", "one", "a", "b", "pre", "c", "post"],
        ),
        (
            "last",
            Position::Last,
            vec!["first", "one", "a", "b", "pre", "c", "post", "last"],
        ),
        // An index past the end means last.
        (
            "over",
            Position::Index(99),
            vec!["first", "one", "a", "b", "pre", "c", "post", "last", "over"],
        ),
    ];
    for (name, at, expected) in cases {
        assert_eq!(
            tree.apply(add(name, "root", "children", at)),
            Ok(Applied::Added),
            "{name}"
        );
        assert_eq!(children_of(&tree, "root", "children"), expected, "{name}");
    }
}

#[test]
fn move_puts_the_node_where_the_position_says() {
    let mut tree = tree();
    tree.place(node("inner"), &id("root"), slot("body"))
        .unwrap();
    for name in ["x", "y"] {
        tree.place(node(name), &id("inner"), slot("body")).unwrap();
    }
    // root.children is [a, b, c]; inner.body is [x, y].
    assert_eq!(
        tree.apply(move_to("c", "root", "children", Position::First)),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["c", "a", "b"]);
    assert_eq!(
        tree.apply(move_to("c", "root", "children", Position::Last)),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["a", "b", "c"]);
    assert_eq!(
        tree.apply(move_to("a", "root", "children", Position::After(id("c")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["b", "c", "a"]);
    assert_eq!(
        tree.apply(move_to("a", "root", "children", Position::Before(id("b")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["a", "b", "c"]);
    assert_eq!(
        tree.apply(move_to("b", "root", "children", Position::Index(0))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["b", "a", "c"]);
    // An index past the end means last.
    assert_eq!(
        tree.apply(move_to("b", "root", "children", Position::Index(99))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["a", "c", "b"]);
    // A named position resolves against the target slot, across slots.
    assert_eq!(
        tree.apply(move_to("x", "root", "children", Position::Before(id("a")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["x", "a", "c", "b"]);
    assert_eq!(children_of(&tree, "inner", "body"), ["y"]);
    assert_eq!(
        tree.apply(move_to("y", "root", "children", Position::After(id("c")))),
        Ok(Applied::Moved)
    );
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["x", "a", "c", "y", "b"]
    );
    assert!(children_of(&tree, "inner", "body").is_empty());
    assert_eq!(
        tree.apply(move_to("y", "inner", "body", Position::Last)),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "inner", "body"), ["y"]);
    assert_eq!(children_of(&tree, "root", "children"), ["x", "a", "c", "b"]);
}

#[test]
fn a_move_within_its_own_slot_resolves_after_the_node_is_taken_out() {
    let mut tree = tree();
    // Whether the node stood before or after x, After(x) lands it right
    // after x.
    assert_eq!(
        tree.apply(move_to("c", "root", "children", Position::After(id("a")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["a", "c", "b"]);
    assert_eq!(
        tree.apply(move_to("a", "root", "children", Position::After(id("c")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["c", "a", "b"]);
    // Before(x) lands it right before x, again from either side.
    assert_eq!(
        tree.apply(move_to("b", "root", "children", Position::Before(id("c")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["b", "c", "a"]);
    assert_eq!(
        tree.apply(move_to("a", "root", "children", Position::Before(id("b")))),
        Ok(Applied::Moved)
    );
    assert_eq!(children_of(&tree, "root", "children"), ["a", "b", "c"]);
}

#[test]
fn a_position_naming_a_node_outside_the_slot_is_refused_and_the_tree_is_unchanged() {
    let mut tree = tree();
    tree.place(node("inner"), &id("root"), slot("body"))
        .unwrap();
    tree.place(node("x"), &id("inner"), slot("body")).unwrap();
    let before = snapshot(&tree);
    for at in [
        // In the tree, but in another slot.
        Position::Before(id("x")),
        Position::After(id("x")),
        // In the tree, in another slot of the same parent.
        Position::Before(id("inner")),
        // In the tree, but never in a slot: the root.
        Position::After(id("root")),
        // Not in the tree at all.
        Position::Before(id("ghost")),
        Position::After(id("ghost")),
    ] {
        assert!(
            tree.place_at(node("fresh"), &id("root"), slot("children"), at.clone())
                .is_err(),
            "place_at {at:?}"
        );
        assert!(
            tree.apply(add("fresh", "root", "children", at.clone()))
                .is_err(),
            "add {at:?}"
        );
        assert!(
            tree.apply(move_to("a", "root", "children", at.clone()))
                .is_err(),
            "move {at:?}"
        );
    }
    assert_eq!(snapshot(&tree), before);

    // The position resolves against the slot as it is then: once `x` is in
    // root.children, naming it works.
    tree.apply(move_to("x", "root", "children", Position::Last))
        .unwrap();
    tree.place_at(
        node("pre"),
        &id("root"),
        slot("children"),
        Position::Before(id("x")),
    )
    .unwrap();
    assert_eq!(
        children_of(&tree, "root", "children"),
        ["a", "b", "c", "pre", "x"]
    );
}

#[test]
fn a_move_beside_itself_is_refused_and_the_tree_is_unchanged() {
    let mut tree = tree();
    let before = snapshot(&tree);
    for at in [Position::Before(id("a")), Position::After(id("a"))] {
        assert!(
            tree.apply(move_to("a", "root", "children", at.clone()))
                .is_err(),
            "{at:?}"
        );
    }
    // A `place_at` or an `add` whose own name is named is refused too: the
    // node is not in the slot, or its ID is already taken.
    assert!(
        tree.place_at(
            node("a"),
            &id("root"),
            slot("children"),
            Position::Before(id("a"))
        )
        .is_err()
    );
    assert!(
        tree.apply(add("d", "root", "children", Position::After(id("d"))))
            .is_err()
    );
    assert_eq!(snapshot(&tree), before);
}

#[test]
fn the_user_layer_records_the_position_as_it_was_asked() {
    let mut layers = Layers::new(tree());
    let ops = [
        move_to("c", "root", "children", Position::First),
        add("extra", "root", "children", Position::Before(id("a"))),
        move_to("b", "root", "children", Position::After(id("extra"))),
        move_to("c", "root", "children", Position::Last),
    ];
    for op in &ops {
        layers.apply(op.clone()).unwrap();
    }
    // A refused position is not recorded.
    assert!(
        layers
            .apply(move_to(
                "a",
                "root",
                "children",
                Position::Before(id("ghost"))
            ))
            .is_err()
    );
    assert_eq!(layers.user_layer(), ops);
    assert_eq!(
        children_of(layers.current(), "root", "children"),
        ["extra", "b", "a", "c"]
    );
    // A reset drops them like any other operation.
    assert_eq!(layers.reset(), 4);
    assert_eq!(
        children_of(layers.current(), "root", "children"),
        ["a", "b", "c"]
    );
}

#[test]
fn each_position_encodes_to_its_key_and_decodes_back() {
    let ops = vec![
        move_to("a", "root", "children", Position::Index(2)),
        move_to("b", "root", "children", Position::First),
        move_to("c", "root", "children", Position::Before(id("x"))),
        move_to("d", "root", "children", Position::After(id("y"))),
        move_to("e", "root", "children", Position::Last),
        add("f", "root", "children", Position::Index(0)),
        add("g", "root", "children", Position::First),
        add("h", "root", "children", Position::Before(id("x"))),
        add("i", "root", "children", Position::After(id("y"))),
        add("j", "root", "children", Position::Last),
    ];
    let text = encode(&ops);
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["version"], json!(1));
    let encoded = file["operations"].as_array().unwrap();
    let position_keys = ["index", "first", "before", "after"];
    // An operation carries exactly the key of its position, and no other.
    let only_key = |op: &serde_json::Value, key: &str, value: serde_json::Value| {
        for other in position_keys {
            if other == key {
                assert_eq!(op.get(other), Some(&value), "{op}");
            } else {
                assert!(op.get(other).is_none(), "{op} must not have {other}");
            }
        }
    };
    only_key(&encoded[0], "index", json!(2));
    only_key(&encoded[1], "first", json!(true));
    only_key(&encoded[2], "before", json!("x"));
    only_key(&encoded[3], "after", json!("y"));
    only_key(&encoded[5], "index", json!(0));
    only_key(&encoded[6], "first", json!(true));
    only_key(&encoded[7], "before", json!("x"));
    only_key(&encoded[8], "after", json!("y"));
    // A last position is written as no key at all.
    for entry in [&encoded[4], &encoded[9]] {
        for key in position_keys {
            assert!(
                entry.get(key).is_none(),
                "a last position writes no {key}: {entry}"
            );
        }
    }
    assert_eq!(decode(&text), Ok(ops));
}

#[test]
fn named_positions_decode_to_the_position_they_name() {
    let text = r#"{
  "version": 1,
  "operations": [
    {"op": "move", "node": "a", "parent": "root", "slot": "children", "first": true},
    {"op": "move", "node": "b", "parent": "root", "slot": "children", "before": "x"},
    {"op": "move", "node": "c", "parent": "root", "slot": "children", "after": "y"},
    {"op": "move", "node": "d", "parent": "root", "slot": "children", "index": 2},
    {"op": "move", "node": "e", "parent": "root", "slot": "children"},
    {"op": "add", "node": "f", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "children", "first": true},
    {"op": "add", "node": "g", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "children", "before": "x"},
    {"op": "add", "node": "h", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "children", "after": "y"},
    {"op": "add", "node": "i", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "children", "index": 0},
    {"op": "add", "node": "j", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "children"}
  ]
}"#;
    assert_eq!(
        decode(text),
        Ok(vec![
            move_to("a", "root", "children", Position::First),
            move_to("b", "root", "children", Position::Before(id("x"))),
            move_to("c", "root", "children", Position::After(id("y"))),
            move_to("d", "root", "children", Position::Index(2)),
            move_to("e", "root", "children", Position::Last),
            add("f", "root", "children", Position::First),
            add("g", "root", "children", Position::Before(id("x"))),
            add("h", "root", "children", Position::After(id("y"))),
            add("i", "root", "children", Position::Index(0)),
            add("j", "root", "children", Position::Last),
        ])
    );
}

#[test]
fn a_feature_12_file_still_decodes() {
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
    let ops = vec![
        Operation::Add {
            node: Node {
                id: id("extra"),
                component: ComponentId::parse("layout_tour.card").unwrap(),
                extension: "layout_tour".into(),
                props: Props::new(),
            },
            parent: id("root"),
            slot: slot("children"),
            at: Position::Last,
        },
        Operation::Remove { node: id("old") },
        move_to("a", "right", "children", Position::Index(2)),
        move_to("b", "right", "children", Position::Last),
        Operation::SetHidden {
            node: id("d"),
            hidden: true,
        },
        Operation::SetHidden {
            node: id("d"),
            hidden: false,
        },
    ];
    // `index` reads as an Index position, no key as Last, and an old add
    // has no position, as an add never had one.
    assert_eq!(decode(pinned), Ok(ops.clone()));
    assert_eq!(encode(&ops), pinned);
}

#[test]
fn a_file_with_two_positions_or_a_bad_one_cannot_be_read() {
    for op_json in [
        // More than one position on one operation.
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "index": 0, "first": true}"#,
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "index": 0, "before": "x"}"#,
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "first": true, "after": "x"}"#,
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "before": "x", "after": "y"}"#,
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "index": 0, "first": true, "after": "x"}"#,
        r#"{"op": "add", "node": "a", "component": "tour.card", "extension": "tour", "parent": "b", "slot": "c", "index": 0, "after": "x"}"#,
        r#"{"op": "add", "node": "a", "component": "tour.card", "extension": "tour", "parent": "b", "slot": "c", "first": true, "before": "x"}"#,
        // A named position that is not a node ID.
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "before": "Bad Id"}"#,
        r#"{"op": "move", "node": "a", "parent": "b", "slot": "c", "after": 7}"#,
        r#"{"op": "add", "node": "a", "component": "tour.card", "extension": "tour", "parent": "b", "slot": "c", "before": true}"#,
    ] {
        let text = format!(r#"{{"version": 1, "operations": [{op_json}]}}"#);
        assert!(decode(&text).is_err(), "{op_json} must be refused");
    }
}
