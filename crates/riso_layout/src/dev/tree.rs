//! Component and node IDs, and the window's single root.

use std::{error::Error, fmt};

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
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// One placed component.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: NodeId,
    pub component: ComponentId,
}

/// The main window's layout.
#[derive(Default)]
pub struct Tree {
    root: Option<Node>,
}

impl Tree {
    pub fn root(&self) -> Option<&Node> {
        self.root.as_ref()
    }

    pub fn place_root(&mut self, node: Node) -> Result<(), RootTaken> {
        if let Some(root) = &self.root {
            tracing::info!(node = %node.id, component = %node.component, root = %root.id, "a root is already placed");
            return Err(RootTaken);
        }
        tracing::debug!(node = %node.id, component = %node.component, "root placed");
        self.root = Some(node);
        Ok(())
    }
}

/// Refusal to replace the root already placed.
#[derive(Debug)]
pub struct RootTaken;

impl fmt::Display for RootTaken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a root is already placed")
    }
}
impl Error for RootTaken {}

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        tree.place_root(node("root")).unwrap();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        let error = tree.place_root(node("second")).unwrap_err();
        assert_eq!(tree.root().unwrap().id.as_str(), "root");
        assert!(error.to_string().contains("root"));
        let _: &dyn std::error::Error = &error;
    }
}
