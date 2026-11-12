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

/// One placed component.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub component: ComponentId,
    /// The name of the extension that placed the node. The shell checks the name.
    pub extension: String,
}

/// The main window's layout.
#[derive(Default)]
pub struct Tree {
    root: Option<Node>,
    hidden: BTreeSet<NodeId>,
    slots: Vec<(NodeId, SlotName, Vec<Node>)>,
}

impl Tree {
    /// Changes only this node's flag; descendants keep their own flags.
    pub fn set_hidden(&mut self, node: &NodeId, hidden: bool) -> Result<bool, HideError> {
        if !self.contains(node) {
            tracing::info!(node = %node, "the node is not in the tree");
            return Err(HideError::MissingNode);
        }
        if self.root.as_ref().is_some_and(|root| &root.id == node) {
            tracing::info!(node = %node, "the root cannot be hidden or shown");
            return Err(HideError::Root);
        }
        if self.is_hidden(node) == hidden {
            tracing::debug!(node = %node, hidden, "already in that state");
            return Ok(false);
        }
        if hidden {
            self.hidden.insert(node.clone());
            tracing::debug!(node = %node, "node hidden");
        } else {
            self.hidden.remove(node);
            tracing::debug!(node = %node, "node shown");
        }
        Ok(true)
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
        if let Some((_, _, nodes)) = self
            .slots
            .iter_mut()
            .find(|(p, s, _)| p == parent && s == &slot)
        {
            nodes.push(node);
        } else {
            self.slots.push((parent.clone(), slot, vec![node]));
        }
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

    fn contains(&self, id: &NodeId) -> bool {
        self.root.as_ref().is_some_and(|node| &node.id == id)
            || self
                .slots
                .iter()
                .any(|(_, _, nodes)| nodes.iter().any(|node| &node.id == id))
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
        };
        tree.place_root(node("root")).unwrap();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        assert_eq!(tree.root().unwrap().extension, "placer");
        let error = tree.place_root(node("second")).unwrap_err();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        assert!(error.to_string().contains("root"));
        let _: &dyn std::error::Error = &error;
    }

    fn node(id: &str) -> Node {
        Node {
            id: NodeId::parse(id).unwrap(),
            component: ComponentId::parse("tour.card").unwrap(),
            extension: "placer".into(),
        }
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
