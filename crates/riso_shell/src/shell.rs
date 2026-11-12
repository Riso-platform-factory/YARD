//! Component factories and the root view kept by the main window.

use gpui::{
    AnyView, App, AppContext, Axis, Context, Entity, Global, IntoElement, ParentElement, Render,
    Styled, Window, div,
};
use riso_layout::{ComponentId, Node, NodeId, SlotName, Tree};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    rc::Rc,
};

type Factory = Rc<dyn Fn(&NodeId, Slots, &mut Window, &mut App) -> AnyView>;

#[derive(Clone)]
struct Component {
    slots: Vec<SlotName>,
    create: Factory,
}

type SlotViews = Vec<(SlotName, Vec<AnyView>)>;

/// A node's shown children, filled before the first render.
#[derive(Clone)]
pub struct Slots(Rc<RefCell<SlotViews>>);

impl Slots {
    pub fn views(&self, slot: &str) -> Vec<AnyView> {
        self.0
            .borrow()
            .iter()
            .find(|(name, _)| name.as_str() == slot)
            .map(|(_, views)| views.clone())
            .unwrap_or_default()
    }
}

struct Registry {
    extensions: BTreeSet<String>,
    components: BTreeMap<ComponentId, Component>,
    tree: Tree,
}
impl Default for Registry {
    fn default() -> Self {
        let components = [
            ("riso.row", Axis::Horizontal),
            ("riso.column", Axis::Vertical),
        ]
        .into_iter()
        .map(|(name, axis)| {
            (
                ComponentId::parse(name).unwrap(),
                Component {
                    slots: vec![SlotName::parse("children").unwrap()],
                    create: Rc::new(move |node, slots, _, cx| {
                        cx.new(|_| Split {
                            node: node.clone(),
                            axis,
                            slots,
                            shares: Rc::new(RefCell::new(Vec::new())),
                        })
                        .into()
                    }),
                },
            )
        })
        .collect();
        Self {
            extensions: BTreeSet::new(),
            components,
            tree: Tree::default(),
        }
    }
}
impl Global for Registry {}

struct Split {
    node: NodeId,
    shares: Rc<RefCell<Vec<f32>>>,
    axis: Axis,
    slots: Slots,
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        riso_ui::axis(
            gpui::SharedString::from(self.node.to_string()),
            self.axis,
            self.shares.clone(),
            self.slots
                .views("children")
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect(),
        )
    }
}

/// The application-assigned namespace for an extension's contributions.
#[derive(Clone)]
pub struct Extension {
    name: String,
}

impl Extension {
    /// Creates an extension handle. Called only by the startup.
    pub fn new(cx: &mut App, name: &str) -> Option<Extension> {
        if name == "riso" {
            log::error!("riso is the shell's own namespace");
            return None;
        }
        // An extension name follows the same slug rule as a node ID.
        if let Err(error) = NodeId::parse(name) {
            log::error!("invalid extension name {name:?}: {error}");
            return None;
        }
        if !cx
            .default_global::<Registry>()
            .extensions
            .insert(name.into())
        {
            log::error!("extension {name} is already initialized");
            return None;
        }
        Some(Self { name: name.into() })
    }

    /// Registers the first factory for a valid component ID.
    pub fn register(
        &self,
        cx: &mut App,
        component: &str,
        slots: &[&str],
        create: impl Fn(&NodeId, Slots, &mut Window, &mut App) -> AnyView + 'static,
    ) {
        let _group =
            tracing::info_span!("component-register", component, slots = slots.join(",")).entered();
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
                return;
            }
        };
        if component.as_str().split('.').next() != Some(self.name.as_str()) {
            log::error!(
                "extension {} cannot register component {component}",
                self.name
            );
            return;
        }
        let mut names = Vec::new();
        for slot in slots {
            let name = match SlotName::parse(slot) {
                Ok(name) => name,
                Err(error) => {
                    log::error!("invalid slot name {slot:?}: {error}");
                    return;
                }
            };
            if names.contains(&name) {
                log::error!("component {component} declares slot {name} twice");
                return;
            }
            names.push(name);
        }
        match cx.default_global::<Registry>().components.entry(component) {
            Entry::Vacant(entry) => {
                entry.insert(Component {
                    slots: names,
                    create: Rc::new(create),
                });
                tracing::debug!("registered");
            }
            Entry::Occupied(entry) => {
                log::error!("component {} is already registered", entry.key())
            }
        }
    }

    /// Places a valid root, whether or not its component is registered yet.
    pub fn place_root(&self, cx: &mut App, node: &str, component: &str) {
        let _group = tracing::info_span!("root-place", node, component).entered();
        let id = match NodeId::parse(node) {
            Ok(id) => id,
            Err(error) => {
                log::error!("invalid node ID {node:?}: {error}");
                return;
            }
        };
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
                return;
            }
        };
        if component.as_str().split('.').next() != Some(self.name.as_str())
            && component.as_str().split('.').next() != Some("riso")
        {
            log::error!("extension {} cannot place component {component}", self.name);
            return;
        }
        let _ = cx.default_global::<Registry>().tree.place_root(Node {
            id,
            component,
            extension: self.name.clone(),
        });
    }

    /// Places a node under an already placed parent.
    pub fn place(&self, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str) {
        let _group = tracing::info_span!("node-place", node, component, parent, slot).entered();
        let id = match NodeId::parse(node) {
            Ok(id) => id,
            Err(error) => {
                log::error!("invalid node ID {node:?}: {error}");
                return;
            }
        };
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
                return;
            }
        };
        if component.as_str().split('.').next() != Some(self.name.as_str())
            && component.as_str().split('.').next() != Some("riso")
        {
            log::error!("extension {} cannot place component {component}", self.name);
            return;
        }
        let parent = match NodeId::parse(parent) {
            Ok(parent) => parent,
            Err(error) => {
                log::error!("invalid parent ID {parent:?}: {error}");
                return;
            }
        };
        let slot = match SlotName::parse(slot) {
            Ok(slot) => slot,
            Err(error) => {
                log::error!("invalid slot name {slot:?}: {error}");
                return;
            }
        };
        let _ = cx.default_global::<Registry>().tree.place(
            Node {
                id,
                component,
                extension: self.name.clone(),
            },
            &parent,
            slot,
        );
    }
}

/// The window's root and all created views, retained for its lifetime.
pub struct Shell {
    root: Option<AnyView>,
    _views: Vec<AnyView>,
}

/// Creates the shown nodes once, parent before children, when the window opens.
pub fn new_shell(window: &mut Window, cx: &mut App) -> Entity<Shell> {
    let root = cx.default_global::<Registry>().tree.root().cloned();
    let _group = tracing::info_span!(
        "shell-create",
        root = %root.as_ref().map_or("none", |node| node.id.as_str())
    )
    .entered();
    if root.is_none() {
        tracing::info!("no root placed; the window shows the background");
    }
    let mut views = Vec::new();
    let root = root.and_then(|node| create_view(&node, None, window, cx, &mut views));
    cx.new(|_| Shell {
        root,
        _views: views,
    })
}

fn create_view(
    node: &Node,
    parent: Option<(&ComponentId, &[SlotName], &SlotName)>,
    window: &mut Window,
    cx: &mut App,
    views: &mut Vec<AnyView>,
) -> Option<AnyView> {
    let _group = tracing::info_span!(
        "component-create",
        node = %node.id,
        component = %node.component,
        extension = %node.extension,
    )
    .entered();
    if let Some((component, declared, slot)) = parent
        && !declared.contains(slot)
    {
        log::error!(
            "node {} is placed in slot {} not declared by parent component {}",
            node.id,
            slot,
            component
        );
        return None;
    }
    let (component, children) = {
        let registry = cx.default_global::<Registry>();
        let component = registry.components.get(&node.component).cloned();
        let children = registry
            .tree
            .slots(&node.id)
            .map(|(slot, nodes)| (slot.clone(), nodes.to_vec()))
            .collect::<Vec<_>>();
        (component, children)
    };
    let Some(component) = component else {
        log::error!(
            "node {} names unregistered component {}",
            node.id,
            node.component
        );
        return None;
    };
    let slots = Slots(Rc::new(RefCell::new(Vec::new())));
    let view = (component.create)(&node.id, slots.clone(), window, cx);
    views.push(view.clone());
    for (slot, nodes) in children {
        let children = nodes
            .iter()
            .filter_map(|child| {
                create_view(
                    child,
                    Some((&node.component, &component.slots, &slot)),
                    window,
                    cx,
                    views,
                )
            })
            .collect();
        slots.0.borrow_mut().push((slot, children));
    }
    if let Some(axis) = match node.component.as_str() {
        "riso.row" => Some("row"),
        "riso.column" => Some("column"),
        _ => None,
    } {
        tracing::debug!(
            axis,
            children = slots.views("children").len(),
            "split created"
        );
    }
    Some(view)
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering shell");
        window.set_rem_size(riso_ui::ui_font_size());
        div()
            .size_full()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .bg(riso_ui::palette(cx).background)
            .font(riso_ui::ui_font())
            .children(self.root.clone())
    }
}
