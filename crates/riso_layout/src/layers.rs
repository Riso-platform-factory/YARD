//! The startup default and the ordered operations applied over it.

use crate::{Applied, OpError, Operation, Tree};

/// Keeps the startup tree apart from runtime layout operations.
pub struct Layers {
    default: Tree,
    user: Vec<Operation>,
    current: Tree,
    default_fixed: bool,
}

impl Layers {
    pub fn new(tree: Tree) -> Self {
        Self {
            current: tree.clone(),
            default: tree,
            user: Vec::new(),
            default_fixed: false,
        }
    }

    pub fn current(&self) -> &Tree {
        &self.current
    }

    /// Used for startup placement before fixing the default.
    pub fn current_mut(&mut self) -> Option<&mut Tree> {
        (!self.default_fixed).then_some(&mut self.current)
    }

    pub fn set_default(&mut self) {
        if self.default_fixed {
            return;
        }
        self.default_fixed = true;
        self.default = self.current.clone();
        self.user.clear();
    }

    pub fn apply(&mut self, op: Operation) -> Result<Applied, OpError> {
        let applied = self.current.apply(op.clone())?;
        self.user.push(op);
        tracing::debug!(user_layer = self.user.len(), "recorded");
        Ok(applied)
    }

    pub fn reset(&mut self) -> usize {
        let dropped = self.user.len();
        if dropped == 0 {
            tracing::debug!("nothing to reset");
            return 0;
        }
        self.current = self.default.clone();
        self.user.clear();
        tracing::debug!(dropped, "user layer dropped");
        dropped
    }

    pub fn user_layer(&self) -> &[Operation] {
        &self.user
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComponentId, Node, NodeId, SlotName};

    fn id(value: &str) -> NodeId {
        NodeId::parse(value).unwrap()
    }

    fn node(value: &str) -> Node {
        Node {
            id: id(value),
            component: ComponentId::parse("tour.card").unwrap(),
            extension: "tour".into(),
            props: crate::Props::new(),
        }
    }

    fn tree() -> Tree {
        let mut tree = Tree::default();
        tree.place_root(node("root")).unwrap();
        for (child, parent, slot) in [
            ("left", "root", "body"),
            ("right", "root", "body"),
            ("aside", "root", "side"),
            ("inner", "left", "body"),
        ] {
            tree.place(node(child), &id(parent), SlotName::parse(slot).unwrap())
                .unwrap();
        }
        tree.set_hidden(&id("inner"), true).unwrap();
        tree
    }

    type Walk = Vec<(Node, bool, Vec<(SlotName, Vec<Node>)>)>;

    fn walk(tree: &Tree) -> Walk {
        fn visit(tree: &Tree, node: &Node, out: &mut Walk) {
            out.push((
                node.clone(),
                tree.is_hidden(&node.id),
                tree.slots(&node.id)
                    .map(|(slot, children)| (slot.clone(), children.to_vec()))
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
    fn records_only_accepted_operations_in_order() {
        let mut layers = Layers::new(tree());
        let operations = [
            Operation::SetHidden {
                node: id("left"),
                hidden: true,
            },
            Operation::Move {
                node: id("right"),
                parent: id("left"),
                slot: SlotName::parse("body").unwrap(),
                index: 0,
            },
            Operation::SetHidden {
                node: id("left"),
                hidden: false,
            },
            // An accepted no-op is still a successful operation.
            Operation::SetHidden {
                node: id("left"),
                hidden: false,
            },
        ];
        for operation in &operations {
            layers.apply(operation.clone()).unwrap();
            let before = walk(layers.current());
            assert_eq!(
                layers.apply(Operation::Remove { node: id("root") }),
                Err(OpError::Root)
            );
            assert_eq!(walk(layers.current()), before);
        }
        assert_eq!(layers.user_layer(), operations);
    }

    #[test]
    fn reset_restores_the_exact_default_walk() {
        let tree = tree();
        let expected = walk(&tree);
        let mut layers = Layers::new(tree);
        for operation in [
            Operation::SetHidden {
                node: id("inner"),
                hidden: false,
            },
            Operation::Move {
                node: id("aside"),
                parent: id("right"),
                slot: SlotName::parse("body").unwrap(),
                index: 0,
            },
            Operation::Remove { node: id("left") },
            Operation::Add {
                node: node("added"),
                parent: id("root"),
                slot: SlotName::parse("other").unwrap(),
            },
        ] {
            layers.apply(operation).unwrap();
        }
        assert_ne!(walk(layers.current()), expected);
        assert_eq!(layers.reset(), 4);
        assert_eq!(walk(layers.current()), expected);
        assert!(layers.user_layer().is_empty());
    }

    #[test]
    fn empty_reset_changes_nothing() {
        let mut layers = Layers::new(tree());
        let expected = walk(layers.current());
        assert_eq!(layers.reset(), 0);
        assert_eq!(walk(layers.current()), expected);
        assert!(layers.user_layer().is_empty());
        let mut empty = Layers::new(Tree::default());
        assert_eq!(empty.reset(), 0);
        assert!(empty.current().root().is_none());
    }

    #[test]
    fn startup_mutation_closes_when_the_default_is_fixed() {
        let mut layers = Layers::new(Tree::default());
        *layers.current_mut().unwrap() = tree();
        layers.set_default();
        assert!(layers.current_mut().is_none());
        layers
            .apply(Operation::Remove { node: id("left") })
            .unwrap();
        layers.reset();
        assert!(layers.current_mut().is_none());
    }

    #[test]
    fn fixing_default_again_preserves_the_first_default_and_user_layer() {
        let mut layers = Layers::new(tree());
        let expected = walk(layers.current());
        layers.set_default();
        layers
            .apply(Operation::Remove { node: id("left") })
            .unwrap();
        layers.set_default();
        assert_eq!(layers.user_layer().len(), 1);
        assert_eq!(layers.reset(), 1);
        assert_eq!(walk(layers.current()), expected);
    }

    #[test]
    fn set_default_clears_the_layer_and_fixes_startup_placements() {
        let mut layers = Layers::new(Tree::default());
        *layers.current_mut().unwrap() = tree();
        layers
            .apply(Operation::Remove { node: id("right") })
            .unwrap();
        let expected = walk(layers.current());
        layers.set_default();
        assert!(layers.user_layer().is_empty());
        layers
            .apply(Operation::Remove { node: id("left") })
            .unwrap();
        assert_eq!(layers.reset(), 1);
        assert_eq!(walk(layers.current()), expected);
    }
}
