//! Component and node IDs, and the window's tree of named slots.

use std::{collections::BTreeSet, error::Error, fmt};

/// A broken ID rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError(&'static str);

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl Error for IdError {}

fn validate_slug(value: &str) -> Result<(), IdError> {
    let Some(first) = value.bytes().next() else {
        return Err(IdError("a slug must not be empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(IdError("a slug must start with a lowercase ASCII letter"));
    }
    if !value
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
    {
        return Err(IdError(
            "a slug allows only lowercase ASCII letters, digits, '-' and '_'",
        ));
    }
    Ok(())
}

/// A component type, named by its extension and component slugs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentId(String);

impl ComponentId {
    pub fn parse(value: &str) -> Result<Self, IdError> {
        let mut parts = value.split('.');
        let extension = parts.next().unwrap();
        let component = parts.next().ok_or(IdError(
            "a component ID must be exactly two slugs joined by one '.'",
        ))?;
        if parts.next().is_some() {
            return Err(IdError(
                "a component ID must be exactly two slugs joined by one '.'",
            ));
        }
        validate_slug(extension)?;
        validate_slug(component)?;
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The slug naming one placed instance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(String);

impl NodeId {
    pub fn parse(value: &str) -> Result<Self, IdError> {
        validate_slug(value)?;
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The slug naming a component's slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotName(String);

impl SlotName {
    pub fn parse(value: &str) -> Result<Self, IdError> {
        validate_slug(value)?;
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SlotName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A node's untyped configuration object.
pub type Props = serde_json::Map<String, serde_json::Value>;

/// One placed component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub component: ComponentId,
    /// The name of the extension that placed the node. The shell checks the name.
    pub extension: String,
    pub props: Props,
}

/// A runtime change to the layout tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Add {
        node: Node,
        parent: NodeId,
        slot: SlotName,
    },
    Remove {
        node: NodeId,
    },
    Move {
        node: NodeId,
        parent: NodeId,
        slot: SlotName,
        index: usize,
    },
    SetHidden {
        node: NodeId,
        hidden: bool,
    },
    SetProps {
        node: NodeId,
        props: Props,
    },
}

/// What an accepted [`Operation`] changed in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    Added,
    Removed(Vec<NodeId>),
    Moved,
    SetHidden(bool),
    PropsSet,
}

/// An [`Operation`] refused without changing the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    DuplicateId,
    MissingParent,
    MissingNode,
    Root,
    Cycle,
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DuplicateId => "the node ID is already in the tree",
            Self::MissingParent => "the parent is not in the tree",
            Self::MissingNode => "the node is not in the tree",
            Self::Root => "the root cannot be removed, moved, hidden, or shown",
            Self::Cycle => "moving a node into itself or its subtree would make a cycle",
        })
    }
}
impl Error for OpError {}

/// The main window's layout.
#[derive(Default, Clone)]
pub struct Tree {
    root: Option<Node>,
    hidden: BTreeSet<NodeId>,
    slots: Vec<(NodeId, SlotName, Vec<Node>)>,
}

impl Tree {
    /// Checks the model's rules and applies one operation.
    pub fn apply(&mut self, op: Operation) -> Result<Applied, OpError> {
        match op {
            Operation::Add { node, parent, slot } => {
                if self.contains(&node.id) {
                    tracing::info!(
                        node = %node.id,
                        component = %node.component,
                        extension = %node.extension,
                        parent = %parent,
                        slot = %slot,
                        "the node ID is already in the tree",
                    );
                    return Err(OpError::DuplicateId);
                }
                if !self.contains(&parent) {
                    tracing::info!(
                        node = %node.id,
                        component = %node.component,
                        extension = %node.extension,
                        parent = %parent,
                        slot = %slot,
                        "the parent is not in the tree",
                    );
                    return Err(OpError::MissingParent);
                }
                tracing::debug!(
                    node = %node.id,
                    component = %node.component,
                    extension = %node.extension,
                    parent = %parent,
                    slot = %slot,
                    "node added",
                );
                self.insert_into_slot(&parent, slot, usize::MAX, node);
                Ok(Applied::Added)
            }
            Operation::Remove { node } => {
                if !self.contains(&node) {
                    tracing::info!(node = %node, "the node is not in the tree");
                    return Err(OpError::MissingNode);
                }
                if self.is_root(&node) {
                    tracing::info!(node = %node, "the root cannot be removed");
                    return Err(OpError::Root);
                }
                let mut removed = Vec::new();
                self.collect_subtree(&node, &mut removed);
                for (_, _, nodes) in &mut self.slots {
                    nodes.retain(|child| child.id != node);
                }
                self.slots
                    .retain(|(owner, _, nodes)| !removed.contains(owner) && !nodes.is_empty());
                for id in &removed {
                    self.hidden.remove(id);
                }
                tracing::debug!(node = %node, removed = removed.len(), "node removed");
                Ok(Applied::Removed(removed))
            }
            Operation::Move {
                node,
                parent,
                slot,
                index,
            } => {
                if !self.contains(&node) {
                    tracing::info!(
                        node = %node,
                        parent = %parent,
                        slot = %slot,
                        index,
                        "the node is not in the tree",
                    );
                    return Err(OpError::MissingNode);
                }
                if self.is_root(&node) {
                    tracing::info!(
                        node = %node,
                        parent = %parent,
                        slot = %slot,
                        index,
                        "the root cannot be moved",
                    );
                    return Err(OpError::Root);
                }
                if !self.contains(&parent) {
                    tracing::info!(
                        node = %node,
                        parent = %parent,
                        slot = %slot,
                        index,
                        "the parent is not in the tree",
                    );
                    return Err(OpError::MissingParent);
                }
                if parent == node || self.subtree_contains(&node, &parent) {
                    tracing::info!(
                        node = %node,
                        parent = %parent,
                        slot = %slot,
                        index,
                        "moving a node into itself or its subtree would make a cycle",
                    );
                    return Err(OpError::Cycle);
                }
                let taken = self
                    .slots
                    .iter_mut()
                    .find_map(|(_, _, nodes)| {
                        let pos = nodes.iter().position(|child| child.id == node)?;
                        Some(nodes.remove(pos))
                    })
                    .expect("checked non-root node is in a slot");
                let at = self.insert_into_slot(&parent, slot.clone(), index, taken);
                self.slots.retain(|(_, _, nodes)| !nodes.is_empty());
                tracing::debug!(
                    node = %node,
                    parent = %parent,
                    slot = %slot,
                    index = at,
                    "node moved",
                );
                Ok(Applied::Moved)
            }
            Operation::SetHidden { node, hidden } => {
                if !self.contains(&node) {
                    tracing::info!(node = %node, "the node is not in the tree");
                    return Err(OpError::MissingNode);
                }
                if self.is_root(&node) {
                    tracing::info!(node = %node, "the root cannot be hidden or shown");
                    return Err(OpError::Root);
                }
                if self.is_hidden(&node) == hidden {
                    tracing::debug!(node = %node, hidden, "already in that state");
                    return Ok(Applied::SetHidden(false));
                }
                if hidden {
                    self.hidden.insert(node.clone());
                    tracing::debug!(node = %node, "node hidden");
                } else {
                    self.hidden.remove(&node);
                    tracing::debug!(node = %node, "node shown");
                }
                Ok(Applied::SetHidden(true))
            }
            Operation::SetProps { node, props } => {
                let Some(target) = self.node_mut(&node) else {
                    tracing::info!(node = %node, "the node is not in the tree");
                    return Err(OpError::MissingNode);
                };
                let keys = props
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(",");
                for (key, value) in props {
                    if value.is_null() {
                        target.props.remove(&key);
                    } else {
                        target.props.insert(key, value);
                    }
                }
                tracing::debug!(node = %node, keys, "props set");
                Ok(Applied::PropsSet)
            }
        }
    }

    /// Changes only this node's flag; descendants keep their own flags.
    pub fn set_hidden(&mut self, node: &NodeId, hidden: bool) -> Result<bool, HideError> {
        match self.apply(Operation::SetHidden {
            node: node.clone(),
            hidden,
        }) {
            Ok(Applied::SetHidden(changed)) => Ok(changed),
            Ok(_) => unreachable!(),
            Err(OpError::MissingNode) => Err(HideError::MissingNode),
            Err(OpError::Root) => Err(HideError::Root),
            Err(_) => unreachable!(),
        }
    }

    /// Returns the node's own flag, independently of its ancestors.
    pub fn is_hidden(&self, node: &NodeId) -> bool {
        self.hidden.contains(node)
    }

    pub fn root(&self) -> Option<&Node> {
        self.root.as_ref()
    }

    pub fn place_root(&mut self, node: Node) -> Result<(), PlaceError> {
        if let Some(root) = &self.root {
            tracing::info!(
                node = %node.id,
                component = %node.component,
                extension = %node.extension,
                root = %root.id,
                "a root is already placed",
            );
            return Err(PlaceError::RootTaken);
        }
        tracing::debug!(
            node = %node.id,
            component = %node.component,
            extension = %node.extension,
            "root placed"
        );
        self.root = Some(node);
        Ok(())
    }

    pub fn place(&mut self, node: Node, parent: &NodeId, slot: SlotName) -> Result<(), PlaceError> {
        if self.contains(&node.id) {
            tracing::info!(
                node = %node.id,
                component = %node.component,
                extension = %node.extension,
                parent = %parent,
                slot = %slot,
                "the node ID is already in the tree",
            );
            return Err(PlaceError::DuplicateId);
        }
        if !self.contains(parent) {
            tracing::info!(
                node = %node.id,
                component = %node.component,
                extension = %node.extension,
                parent = %parent,
                slot = %slot,
                "the parent is not in the tree",
            );
            return Err(PlaceError::MissingParent);
        }
        tracing::debug!(
            node = %node.id,
            component = %node.component,
            extension = %node.extension,
            parent = %parent,
            slot = %slot,
            "node placed",
        );
        self.insert_into_slot(parent, slot, usize::MAX, node);
        Ok(())
    }

    pub fn slots<'a>(
        &'a self,
        node: &'a NodeId,
    ) -> impl Iterator<Item = (&'a SlotName, &'a [Node])> {
        self.slots
            .iter()
            .filter(move |(parent, _, _)| parent == node)
            .map(|(_, slot, nodes)| (slot, nodes.as_slice()))
    }

    fn is_root(&self, id: &NodeId) -> bool {
        self.root.as_ref().is_some_and(|root| &root.id == id)
    }

    fn contains(&self, id: &NodeId) -> bool {
        self.is_root(id)
            || self
                .slots
                .iter()
                .any(|(_, _, nodes)| nodes.iter().any(|node| &node.id == id))
    }

    fn node_mut(&mut self, id: &NodeId) -> Option<&mut Node> {
        if let Some(root) = &mut self.root
            && &root.id == id
        {
            return Some(root);
        }
        self.slots
            .iter_mut()
            .flat_map(|(_, _, nodes)| nodes.iter_mut())
            .find(|node| &node.id == id)
    }

    fn subtree_contains(&self, ancestor: &NodeId, candidate: &NodeId) -> bool {
        self.slots(ancestor).any(|(_, children)| {
            children
                .iter()
                .any(|child| &child.id == candidate || self.subtree_contains(&child.id, candidate))
        })
    }

    fn collect_subtree(&self, id: &NodeId, out: &mut Vec<NodeId>) {
        out.push(id.clone());
        for (_, children) in self.slots(id) {
            for child in children {
                self.collect_subtree(&child.id, out);
            }
        }
    }

    fn insert_into_slot(
        &mut self,
        parent: &NodeId,
        slot: SlotName,
        index: usize,
        node: Node,
    ) -> usize {
        if let Some((_, _, nodes)) = self
            .slots
            .iter_mut()
            .find(|(p, s, _)| p == parent && s == &slot)
        {
            let at = index.min(nodes.len());
            nodes.insert(at, node);
            at
        } else {
            self.slots.push((parent.clone(), slot, vec![node]));
            0
        }
    }
}

/// A visibility change refused without changing the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HideError {
    MissingNode,
    Root,
}

impl fmt::Display for HideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingNode => "the node is not in the tree",
            Self::Root => "the root cannot be hidden or shown",
        })
    }
}
impl Error for HideError {}

/// A placement refused without changing the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceError {
    RootTaken,
    DuplicateId,
    MissingParent,
}

impl fmt::Display for PlaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RootTaken => "a root is already placed",
            Self::DuplicateId => "the node ID is already in the tree",
            Self::MissingParent => "the parent is not in the tree",
        })
    }
}
impl Error for PlaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    type Snapshot = Vec<(String, String, String, bool, Vec<(String, Vec<String>)>)>;

    fn snapshot(tree: &Tree) -> Snapshot {
        fn visit(tree: &Tree, node: &Node, out: &mut Snapshot) {
            let slots = tree
                .slots(&node.id)
                .map(|(slot, children)| {
                    (
                        slot.to_string(),
                        children.iter().map(|child| child.id.to_string()).collect(),
                    )
                })
                .collect();
            out.push((
                node.id.to_string(),
                node.component.to_string(),
                node.extension.clone(),
                tree.is_hidden(&node.id),
                slots,
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
            extension: "placer".into(),
            props: Props::new(),
        }
    }

    #[test]
    fn apply_checks_every_rule_and_refusals_leave_an_equal_tree() {
        let mut tree = Tree::default();
        // Empty tree refusals leave it empty.
        for (op, expected) in [
            (
                Operation::Add {
                    node: node("a"),
                    parent: id("root"),
                    slot: slot("body"),
                },
                OpError::MissingParent,
            ),
            (Operation::Remove { node: id("root") }, OpError::MissingNode),
            (
                Operation::Move {
                    node: id("a"),
                    parent: id("root"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::MissingNode,
            ),
            (
                Operation::SetHidden {
                    node: id("a"),
                    hidden: true,
                },
                OpError::MissingNode,
            ),
        ] {
            let before = snapshot(&tree);
            assert_eq!(tree.apply(op), Err(expected));
            assert_eq!(snapshot(&tree), before);
        }

        tree.place_root(node("root")).unwrap();
        assert_eq!(
            tree.apply(Operation::Add {
                node: node("a"),
                parent: id("root"),
                slot: slot("body"),
            }),
            Ok(Applied::Added)
        );
        assert_eq!(
            tree.apply(Operation::Add {
                node: node("b"),
                parent: id("a"),
                slot: slot("body"),
            }),
            Ok(Applied::Added)
        );
        assert_eq!(
            tree.apply(Operation::Add {
                node: node("c"),
                parent: id("b"),
                slot: slot("body"),
            }),
            Ok(Applied::Added)
        );
        assert_eq!(
            tree.apply(Operation::SetHidden {
                node: id("b"),
                hidden: true,
            }),
            Ok(Applied::SetHidden(true))
        );
        assert_eq!(
            tree.apply(Operation::SetHidden {
                node: id("b"),
                hidden: true,
            }),
            Ok(Applied::SetHidden(false))
        );

        let before = snapshot(&tree);
        for (op, expected) in [
            (
                Operation::Add {
                    node: node("root"),
                    parent: id("root"),
                    slot: slot("body"),
                },
                OpError::DuplicateId,
            ),
            (
                Operation::Add {
                    node: node("c"),
                    parent: id("root"),
                    slot: slot("other"),
                },
                OpError::DuplicateId,
            ),
            (
                Operation::Add {
                    node: node("fresh"),
                    parent: id("missing"),
                    slot: slot("body"),
                },
                OpError::MissingParent,
            ),
            (
                Operation::Remove {
                    node: id("missing"),
                },
                OpError::MissingNode,
            ),
            (Operation::Remove { node: id("root") }, OpError::Root),
            (
                Operation::Move {
                    node: id("missing"),
                    parent: id("root"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::MissingNode,
            ),
            (
                Operation::Move {
                    node: id("root"),
                    parent: id("a"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::Root,
            ),
            (
                Operation::Move {
                    node: id("a"),
                    parent: id("missing"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::MissingParent,
            ),
            (
                Operation::Move {
                    node: id("a"),
                    parent: id("a"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::Cycle,
            ),
            (
                Operation::Move {
                    node: id("a"),
                    parent: id("b"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::Cycle,
            ),
            (
                Operation::Move {
                    node: id("a"),
                    parent: id("c"),
                    slot: slot("body"),
                    index: 0,
                },
                OpError::Cycle,
            ),
            (
                Operation::SetHidden {
                    node: id("missing"),
                    hidden: true,
                },
                OpError::MissingNode,
            ),
            (
                Operation::SetHidden {
                    node: id("missing"),
                    hidden: false,
                },
                OpError::MissingNode,
            ),
            (
                Operation::SetHidden {
                    node: id("root"),
                    hidden: true,
                },
                OpError::Root,
            ),
            (
                Operation::SetHidden {
                    node: id("root"),
                    hidden: false,
                },
                OpError::Root,
            ),
        ] {
            assert_eq!(tree.apply(op.clone()), Err(expected.clone()), "{op:?}");
            assert_eq!(snapshot(&tree), before, "{op:?}");
            assert!(!expected.to_string().is_empty());
            let _: &dyn Error = &expected;
        }
    }

    #[test]
    fn move_orders_children_by_index_and_carries_the_subtree_and_flags() {
        let mut tree = Tree::default();
        tree.place_root(node("root")).unwrap();
        for name in ["a", "b", "c"] {
            tree.apply(Operation::Add {
                node: node(name),
                parent: id("root"),
                slot: slot("children"),
            })
            .unwrap();
        }
        tree.apply(Operation::Add {
            node: node("sub"),
            parent: id("b"),
            slot: slot("body"),
        })
        .unwrap();
        tree.apply(Operation::Add {
            node: node("deep"),
            parent: id("sub"),
            slot: slot("body"),
        })
        .unwrap();
        tree.apply(Operation::SetHidden {
            node: id("sub"),
            hidden: true,
        })
        .unwrap();

        let children_of = |tree: &Tree, parent: &str, name: &str| -> Vec<String> {
            tree.slots(&id(parent))
                .find(|(s, _)| s.as_str() == name)
                .map(|(_, nodes)| nodes.iter().map(|n| n.id.to_string()).collect())
                .unwrap_or_default()
        };

        // Reordering within the same slot: front, middle, past-the-end.
        assert_eq!(
            tree.apply(Operation::Move {
                node: id("c"),
                parent: id("root"),
                slot: slot("children"),
                index: 0,
            }),
            Ok(Applied::Moved)
        );
        assert_eq!(children_of(&tree, "root", "children"), ["c", "a", "b"]);

        assert_eq!(
            tree.apply(Operation::Move {
                node: id("c"),
                parent: id("root"),
                slot: slot("children"),
                index: 1,
            }),
            Ok(Applied::Moved)
        );
        assert_eq!(children_of(&tree, "root", "children"), ["a", "c", "b"]);

        assert_eq!(
            tree.apply(Operation::Move {
                node: id("a"),
                parent: id("root"),
                slot: slot("children"),
                index: usize::MAX,
            }),
            Ok(Applied::Moved)
        );
        assert_eq!(children_of(&tree, "root", "children"), ["c", "b", "a"]);

        // Moving across slots carries the whole subtree and keeps hidden flags.
        assert_eq!(
            tree.apply(Operation::Move {
                node: id("b"),
                parent: id("a"),
                slot: slot("body"),
                index: 99,
            }),
            Ok(Applied::Moved)
        );
        assert_eq!(children_of(&tree, "root", "children"), ["c", "a"]);
        assert_eq!(children_of(&tree, "a", "body"), ["b"]);
        assert_eq!(children_of(&tree, "b", "body"), ["sub"]);
        assert_eq!(children_of(&tree, "sub", "body"), ["deep"]);
        assert!(tree.is_hidden(&id("sub")));

        // Inserting ahead of `b` at index 0 in `a.body`.
        assert_eq!(
            tree.apply(Operation::Move {
                node: id("c"),
                parent: id("a"),
                slot: slot("body"),
                index: 0,
            }),
            Ok(Applied::Moved)
        );
        assert_eq!(children_of(&tree, "root", "children"), ["a"]);
        assert_eq!(children_of(&tree, "a", "body"), ["c", "b"]);
    }

    #[test]
    fn remove_drops_the_whole_subtree_and_its_hidden_flags() {
        let mut tree = Tree::default();
        tree.place_root(node("root")).unwrap();
        for (name, parent, slot_name) in [
            ("left", "root", "body"),
            ("right", "root", "body"),
            ("child_a", "left", "body"),
            ("child_b", "left", "side"),
            ("grandchild", "child_a", "body"),
        ] {
            tree.apply(Operation::Add {
                node: node(name),
                parent: id(parent),
                slot: slot(slot_name),
            })
            .unwrap();
        }
        tree.apply(Operation::SetHidden {
            node: id("left"),
            hidden: true,
        })
        .unwrap();
        tree.apply(Operation::SetHidden {
            node: id("grandchild"),
            hidden: true,
        })
        .unwrap();

        assert_eq!(
            tree.apply(Operation::Remove { node: id("left") }),
            Ok(Applied::Removed(vec![
                id("left"),
                id("child_a"),
                id("grandchild"),
                id("child_b"),
            ]))
        );

        let remaining: Vec<_> = snapshot(&tree).into_iter().map(|entry| entry.0).collect();
        assert_eq!(remaining, ["root", "right"]);
        for dropped in ["left", "child_a", "grandchild", "child_b"] {
            assert!(!tree.is_hidden(&id(dropped)));
            assert_eq!(tree.slots(&id(dropped)).count(), 0);
            assert_eq!(
                tree.apply(Operation::Remove { node: id(dropped) }),
                Err(OpError::MissingNode)
            );
        }

        // Removed IDs are free again and start shown.
        assert_eq!(
            tree.apply(Operation::Add {
                node: node("grandchild"),
                parent: id("right"),
                slot: slot("body"),
            }),
            Ok(Applied::Added)
        );
        assert!(!tree.is_hidden(&id("grandchild")));
    }

    #[test]
    fn visibility_flags_start_shown_and_changes_preserve_the_tree() {
        let mut tree = Tree::default();
        let root = NodeId::parse("root").unwrap();
        let child = NodeId::parse("child").unwrap();
        let missing = NodeId::parse("missing").unwrap();
        for hidden in [true, false] {
            assert_eq!(
                tree.set_hidden(&missing, hidden),
                Err(HideError::MissingNode)
            );
        }
        tree.place_root(node("root")).unwrap();
        tree.place(node("child"), &root, SlotName::parse("body").unwrap())
            .unwrap();
        assert!(!tree.is_hidden(&root));
        assert!(!tree.is_hidden(&child));
        assert!(!tree.is_hidden(&missing));
        for hidden in [false, true] {
            assert_eq!(tree.set_hidden(&root, hidden), Err(HideError::Root));
            assert!(!tree.is_hidden(&root));
        }
        assert_eq!(tree.set_hidden(&child, false), Ok(false));
        assert_eq!(tree.set_hidden(&child, true), Ok(true));
        assert!(tree.is_hidden(&child));
        assert_eq!(tree.set_hidden(&child, true), Ok(false));
        assert_eq!(tree.set_hidden(&child, false), Ok(true));
        assert!(!tree.is_hidden(&child));
        assert_eq!(tree.root().unwrap().id, root);
        assert_eq!(tree.slots(&root).next().unwrap().1[0].id, child);
        for error in [HideError::MissingNode, HideError::Root] {
            assert!(!error.to_string().is_empty());
            let _: &dyn Error = &error;
        }
    }

    #[test]
    fn descendants_keep_their_own_flags_while_an_ancestor_is_hidden() {
        let mut tree = Tree::default();
        let root = NodeId::parse("root").unwrap();
        let parent = NodeId::parse("parent").unwrap();
        let child = NodeId::parse("child").unwrap();
        tree.place_root(node("root")).unwrap();
        tree.place(node("parent"), &root, SlotName::parse("body").unwrap())
            .unwrap();
        tree.place(node("child"), &parent, SlotName::parse("body").unwrap())
            .unwrap();
        tree.set_hidden(&parent, true).unwrap();
        assert!(!tree.is_hidden(&child));
        tree.set_hidden(&child, true).unwrap();
        tree.set_hidden(&parent, false).unwrap();
        assert!(tree.is_hidden(&child));
        tree.set_hidden(&parent, true).unwrap();
        tree.set_hidden(&child, false).unwrap();
        assert!(tree.is_hidden(&parent));
        assert!(!tree.is_hidden(&child));
    }

    #[test]
    fn node_ids_accept_every_slug_character() {
        for value in [
            "a",
            "root",
            "a0123456789",
            "abcdefghijklmnopqrstuvwxyz",
            "a-_b",
        ] {
            let id = NodeId::parse(value).unwrap();
            assert_eq!(id.as_str(), value);
            assert_eq!(id.to_string(), value);
        }
    }

    #[test]
    fn node_ids_refuse_every_broken_rule() {
        for value in [
            "",
            "0root",
            "-root",
            "_root",
            "Root",
            "rOot",
            "root.node",
            "root node",
            "root/",
            "røot",
            "root\n",
        ] {
            assert!(NodeId::parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn component_ids_are_exactly_two_slugs() {
        for value in [
            "a.b",
            "layout_tour.card",
            "a0-_.b1_-",
            "abcdefghijklmnopqrstuvwxyz.abcdefghijklmnopqrstuvwxyz",
        ] {
            let id = ComponentId::parse(value).unwrap();
            assert_eq!(id.as_str(), value);
            assert_eq!(id.to_string(), value);
        }
        for value in [
            "", "card", ".card", "tour.", "a.b.c", "a..b", "0a.b", "a.0b", "-a.b", "a._b", "A.b",
            "a.B", "a b.c", "a.b c", "a/b.c", "a.b/", "å.b", "a.ø", "a.b\n",
        ] {
            assert!(ComponentId::parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn errors_describe_the_broken_rule() {
        assert!(NodeId::parse("").unwrap_err().to_string().contains("empty"));
        assert!(
            NodeId::parse("0a")
                .unwrap_err()
                .to_string()
                .contains("start")
        );
        assert!(
            NodeId::parse("a!")
                .unwrap_err()
                .to_string()
                .contains("lowercase ASCII")
        );
        assert!(
            ComponentId::parse("a.b.c")
                .unwrap_err()
                .to_string()
                .contains("exactly two")
        );
        fn is_error(_: &dyn std::error::Error) {}
        is_error(&NodeId::parse("").unwrap_err());
    }

    #[test]
    fn the_first_root_stays_when_a_second_is_refused() {
        let mut tree = Tree::default();
        assert!(tree.root().is_none());
        let node = |id| Node {
            id: NodeId::parse(id).unwrap(),
            component: ComponentId::parse("tour.card").unwrap(),
            extension: "placer".into(),
            props: Props::new(),
        };
        tree.place_root(node("root")).unwrap();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        assert_eq!(tree.root().unwrap().extension, "placer");
        let error = tree.place_root(node("second")).unwrap_err();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        assert!(error.to_string().contains("root"));
        let _: &dyn std::error::Error = &error;
    }

    #[test]
    fn slot_names_use_the_slug_rule() {
        for value in [
            "a",
            "body",
            "a0123456789",
            "abcdefghijklmnopqrstuvwxyz",
            "a-_b",
        ] {
            let slot = SlotName::parse(value).unwrap();
            assert_eq!(slot.as_str(), value);
            assert_eq!(slot.to_string(), value);
        }
        for value in [
            "",
            "0body",
            "-body",
            "_body",
            "Body",
            "bOdy",
            "body.slot",
            "body slot",
            "body/",
            "bødy",
            "body\n",
        ] {
            assert!(SlotName::parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn placement_preserves_slot_and_child_order_and_walks_nested_nodes() {
        let mut tree = Tree::default();
        let root = NodeId::parse("root").unwrap();
        assert_eq!(tree.slots(&root).count(), 0);
        tree.place_root(node("root")).unwrap();
        tree.place(node("left"), &root, SlotName::parse("body").unwrap())
            .unwrap();
        tree.place(node("aside"), &root, SlotName::parse("side").unwrap())
            .unwrap();
        tree.place(node("right"), &root, SlotName::parse("body").unwrap())
            .unwrap();
        tree.place(
            node("inner"),
            &NodeId::parse("left").unwrap(),
            SlotName::parse("body").unwrap(),
        )
        .unwrap();
        assert_eq!(tree.root().unwrap().extension, "placer");
        for (_, children) in tree.slots(&root) {
            assert!(children.iter().all(|node| node.extension == "placer"));
        }
        let slots: Vec<_> = tree
            .slots(&root)
            .map(|(slot, nodes)| {
                (
                    slot.as_str(),
                    nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            slots,
            [("body", vec!["left", "right"]), ("side", vec!["aside"])]
        );
        let mut walk = Vec::new();
        fn visit<'a>(tree: &'a Tree, node: &'a Node, walk: &mut Vec<&'a str>) {
            walk.push(node.id.as_str());
            for (_, nodes) in tree.slots(&node.id) {
                for child in nodes {
                    visit(tree, child, walk);
                }
            }
        }
        visit(&tree, tree.root().unwrap(), &mut walk);
        assert_eq!(walk, ["root", "left", "inner", "right", "aside"]);
        assert_eq!(tree.slots(&NodeId::parse("right").unwrap()).count(), 0);
    }

    #[test]
    fn refused_placements_leave_the_tree_unchanged_and_ids_are_global() {
        let mut tree = Tree::default();
        let root = NodeId::parse("root").unwrap();
        let slot = SlotName::parse("body").unwrap();
        assert_eq!(
            tree.place(node("lost"), &root, slot.clone()),
            Err(PlaceError::MissingParent)
        );
        assert!(tree.root().is_none());
        tree.place_root(node("root")).unwrap();
        tree.place(node("left"), &root, slot.clone()).unwrap();
        for (id, parent, name) in [
            ("root", "root", "body"),
            ("left", "root", "body"),
            ("left", "left", "other"),
        ] {
            assert_eq!(
                tree.place(
                    node(id),
                    &NodeId::parse(parent).unwrap(),
                    SlotName::parse(name).unwrap()
                ),
                Err(PlaceError::DuplicateId)
            );
        }
        assert_eq!(
            tree.place(node("lost"), &NodeId::parse("missing").unwrap(), slot),
            Err(PlaceError::MissingParent)
        );
        assert_eq!(tree.place_root(node("left")), Err(PlaceError::RootTaken));
        assert_eq!(tree.place_root(node("root")), Err(PlaceError::RootTaken));
        assert_eq!(tree.slots(&root).count(), 1);
        assert_eq!(tree.slots(&root).next().unwrap().1.len(), 1);
        assert_eq!(tree.slots(&NodeId::parse("left").unwrap()).count(), 0);
        for error in [
            PlaceError::RootTaken,
            PlaceError::DuplicateId,
            PlaceError::MissingParent,
        ] {
            assert!(!error.to_string().is_empty());
            let _: &dyn std::error::Error = &error;
        }
    }
}
