use serde_json::json;
use yard_layout::{
    Applied, ComponentId, DecodeError, Layers, Node, NodeId, OpError, Operation, Position, Props,
    SlotName, Tree, decode, encode,
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

fn object(value: serde_json::Value) -> Props {
    value.as_object().unwrap().clone()
}

fn props_of(tree: &Tree, name: &str) -> Option<Props> {
    fn find<'a>(tree: &'a Tree, node: &'a Node, name: &str) -> Option<&'a Node> {
        if node.id.as_str() == name {
            return Some(node);
        }
        tree.slots(&node.id)
            .flat_map(|(_, children)| children)
            .find_map(|child| find(tree, child, name))
    }
    find(tree, tree.root()?, name).map(|node| node.props.clone())
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

#[test]
fn set_props_merges_keys_and_null_removes_one() {
    let mut tree = Tree::default();
    tree.place_root(node("root")).unwrap();
    tree.place(node("alpha"), &id("root"), slot("body"))
        .unwrap();
    tree.place(
        Node {
            props: object(json!({"title": "Beta", "tint": "accent"})),
            ..node("beta")
        },
        &id("root"),
        slot("body"),
    )
    .unwrap();

    // A node placed without props has an empty object.
    assert_eq!(props_of(&tree, "alpha"), Some(Props::new()));

    // Setting keys merges: named keys change, the others stay as they were.
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("beta"),
            props: object(json!({"tint": "warning", "count": 3})),
        }),
        Ok(Applied::PropsSet)
    );
    assert_eq!(
        props_of(&tree, "beta"),
        Some(object(
            json!({"title": "Beta", "tint": "warning", "count": 3})
        ))
    );

    // Setting a key to null removes it, so the default is used again. A null for
    // a key that is not there changes nothing.
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("beta"),
            props: object(json!({"tint": null, "absent": null})),
        }),
        Ok(Applied::PropsSet)
    );
    assert_eq!(
        props_of(&tree, "beta"),
        Some(object(json!({"title": "Beta", "count": 3})))
    );

    // The root can have its props set.
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("root"),
            props: object(json!({"title": "Root"})),
        }),
        Ok(Applied::PropsSet)
    );
    assert_eq!(
        props_of(&tree, "root"),
        Some(object(json!({"title": "Root"})))
    );
}

#[test]
fn set_props_is_refused_for_a_node_not_in_the_tree() {
    let mut tree = Tree::default();
    let before = snapshot(&tree);
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("ghost"),
            props: object(json!({"title": "G"})),
        }),
        Err(OpError::MissingNode)
    );
    assert_eq!(snapshot(&tree), before);

    tree.place_root(node("root")).unwrap();
    tree.place(node("alpha"), &id("root"), slot("body"))
        .unwrap();
    let before = snapshot(&tree);
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("ghost"),
            props: object(json!({"title": "G"})),
        }),
        Err(OpError::MissingNode)
    );
    assert_eq!(snapshot(&tree), before);

    // A removed node is not in the tree either.
    tree.apply(Operation::Remove { node: id("alpha") }).unwrap();
    assert_eq!(
        tree.apply(Operation::SetProps {
            node: id("alpha"),
            props: object(json!({"title": "A"})),
        }),
        Err(OpError::MissingNode)
    );
}

#[test]
fn props_stay_through_move_hide_and_show_and_go_with_remove() {
    let mut tree = Tree::default();
    tree.place_root(node("root")).unwrap();
    tree.place(
        Node {
            props: object(json!({"title": "Alpha", "tint": "accent"})),
            ..node("alpha")
        },
        &id("root"),
        slot("children"),
    )
    .unwrap();
    tree.place(node("right"), &id("root"), slot("children"))
        .unwrap();

    tree.place(
        Node {
            props: object(json!({"title": "Inner"})),
            ..node("inner")
        },
        &id("alpha"),
        slot("body"),
    )
    .unwrap();

    let placed = object(json!({"title": "Alpha", "tint": "accent"}));
    for op in [
        Operation::Move {
            node: id("alpha"),
            parent: id("right"),
            slot: slot("body"),
            at: Position::Index(0),
        },
        Operation::SetHidden {
            node: id("alpha"),
            hidden: true,
        },
        Operation::SetHidden {
            node: id("alpha"),
            hidden: false,
        },
        Operation::Move {
            node: id("alpha"),
            parent: id("root"),
            slot: slot("children"),
            at: Position::Last,
        },
    ] {
        assert!(tree.apply(op).is_ok());
        assert_eq!(props_of(&tree, "alpha"), Some(placed.clone()));
        assert_eq!(
            props_of(&tree, "inner"),
            Some(object(json!({"title": "Inner"})))
        );
    }

    // The props go with the node when it is removed, its subtree's too.
    assert_eq!(
        tree.apply(Operation::Remove { node: id("alpha") }),
        Ok(Applied::Removed(vec![id("alpha"), id("inner")]))
    );
    assert_eq!(props_of(&tree, "alpha"), None);
    assert_eq!(props_of(&tree, "inner"), None);
    // Added again under the same ID, the node starts with empty props.
    tree.apply(Operation::Add {
        node: node("alpha"),
        parent: id("root"),
        slot: slot("children"),
        at: Position::Last,
    })
    .unwrap();
    assert_eq!(props_of(&tree, "alpha"), Some(Props::new()));
}

#[test]
fn the_user_layer_records_set_props_and_a_reset_gives_the_defaults_back() {
    let mut default = Tree::default();
    default.place_root(node("root")).unwrap();
    default
        .place(
            Node {
                props: object(json!({"title": "Alpha", "tint": "accent"})),
                ..node("alpha")
            },
            &id("root"),
            slot("children"),
        )
        .unwrap();
    default
        .place(node("beta"), &id("root"), slot("children"))
        .unwrap();
    let mut layers = Layers::new(default);

    let first = Operation::SetProps {
        node: id("alpha"),
        props: object(json!({"tint": "warning"})),
    };
    let second = Operation::SetProps {
        node: id("beta"),
        props: object(json!({"title": "Beta"})),
    };
    assert_eq!(layers.apply(first.clone()), Ok(Applied::PropsSet));
    assert_eq!(layers.apply(second.clone()), Ok(Applied::PropsSet));
    // A refused operation is not recorded.
    assert_eq!(
        layers.apply(Operation::SetProps {
            node: id("ghost"),
            props: object(json!({"title": "G"})),
        }),
        Err(OpError::MissingNode)
    );
    assert_eq!(layers.user_layer(), [first, second]);
    assert_eq!(
        props_of(layers.current(), "alpha"),
        Some(object(json!({"title": "Alpha", "tint": "warning"})))
    );
    assert_eq!(
        props_of(layers.current(), "beta"),
        Some(object(json!({"title": "Beta"})))
    );

    // A reset drops the operations like the others: every node has its default
    // props again.
    assert_eq!(layers.reset(), 2);
    assert!(layers.user_layer().is_empty());
    assert_eq!(
        props_of(layers.current(), "alpha"),
        Some(object(json!({"title": "Alpha", "tint": "accent"})))
    );
    assert_eq!(props_of(layers.current(), "beta"), Some(Props::new()));
}

#[test]
fn add_and_props_encode_and_decode_back_to_the_same_operations() {
    let ops = vec![
        Operation::Add {
            node: Node {
                props: object(json!({"title": "Extra", "count": 2})),
                ..node("extra")
            },
            parent: id("root"),
            slot: slot("children"),
            at: Position::Last,
        },
        Operation::Add {
            node: node("plain"),
            parent: id("root"),
            slot: slot("children"),
            at: Position::Last,
        },
        Operation::SetProps {
            node: id("extra"),
            props: object(json!({"tint": "warning", "gone": null})),
        },
    ];
    let text = encode(&ops);
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    let encoded = file["operations"].as_array().unwrap();
    // An add has a props object when the node was added with props...
    assert_eq!(encoded[0]["props"], json!({"title": "Extra", "count": 2}));
    // ...and none otherwise.
    assert!(encoded[1].get("props").is_none(), "{text}");
    // The new kind is `props`, with `node` and `props`.
    assert_eq!(encoded[2]["op"], json!("props"));
    assert_eq!(encoded[2]["node"], json!("extra"));
    assert_eq!(
        encoded[2]["props"],
        json!({"tint": "warning", "gone": null})
    );
    assert_eq!(decode(&text), Ok(ops));
}

#[test]
fn a_feature_12_file_with_no_props_decodes() {
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
    let ops = decode(pinned).unwrap();
    assert_eq!(ops.len(), 6);
    for op in &ops {
        if let Operation::Add { node, .. } = op {
            assert_eq!(node.props, Props::new());
        }
    }
    assert_eq!(decode(&encode(&ops)), Ok(ops));
}

#[test]
fn props_that_are_not_objects_are_refused() {
    for op_json in [
        r#"{"op": "props", "node": "a", "props": 42}"#,
        r#"{"op": "props", "node": "a", "props": "title"}"#,
        r#"{"op": "props", "node": "a", "props": true}"#,
        r#"{"op": "props", "node": "a", "props": ["x"]}"#,
        r#"{"op": "props", "node": "a"}"#,
        r#"{"op": "props", "node": "a", "props": {}, "extra": 1}"#,
        r#"{"op": "add", "node": "a", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "body", "props": 42}"#,
        r#"{"op": "add", "node": "a", "component": "tour.card", "extension": "tour", "parent": "root", "slot": "body", "props": [1]}"#,
    ] {
        let text = format!(r#"{{"version": 1, "operations": [{op_json}]}}"#);
        assert!(
            matches!(decode(&text), Err(DecodeError::InvalidJson(_))),
            "{op_json} must be refused"
        );
    }
    // The slug rules apply to the props kind too.
    let error =
        decode(r#"{"version": 1, "operations": [{"op": "props", "node": "Bad", "props": {}}]}"#)
            .unwrap_err();
    match error {
        DecodeError::InvalidId { field, .. } => assert_eq!(field, "node"),
        other => panic!("expected InvalidId for node, got {other:?}"),
    }
    // An empty props object is a valid props operation.
    assert_eq!(
        decode(r#"{"version": 1, "operations": [{"op": "props", "node": "a", "props": {}}]}"#),
        Ok(vec![Operation::SetProps {
            node: id("a"),
            props: Props::new(),
        }])
    );
}
