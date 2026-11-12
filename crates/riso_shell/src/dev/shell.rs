//! Component factories and the root view kept by the main window.

use gpui::{
    AnyView, App, AppContext, Axis, Context, Entity, Global, IntoElement, ParentElement, Render,
    Styled, WeakEntity, Window, div,
};
use riso_layout::dev::{ComponentId, Node, NodeId, SlotName, Tree};
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

type SlotViews = Vec<(SlotName, Vec<(NodeId, AnyView)>)>;

/// A node's shown children, filled before the first render.
#[derive(Clone)]
pub struct Slots(Rc<RefCell<SlotViews>>, Rc<RefCell<Tree>>);

impl Slots {
    pub fn nodes(&self, slot: &str) -> Vec<NodeId> {
        self.0
            .borrow()
            .iter()
            .find(|(name, _)| name.as_str() == slot)
            .map(|(_, views)| {
                views
                    .iter()
                    .filter(|(node, _)| !self.1.borrow().is_hidden(node))
                    .map(|(node, _)| node.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn views(&self, slot: &str) -> Vec<AnyView> {
        self.0
            .borrow()
            .iter()
            .find(|(name, _)| name.as_str() == slot)
            .map(|(_, views)| {
                views
                    .iter()
                    .filter(|(node, _)| !self.1.borrow().is_hidden(node))
                    .map(|(_, view)| view.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

struct Registry {
    extensions: BTreeSet<String>,
    components: BTreeMap<ComponentId, Component>,
    tree: Rc<RefCell<Tree>>,
    tabs: Vec<WeakEntity<Tabs>>,
}
impl Default for Registry {
    fn default() -> Self {
        let mut components: BTreeMap<_, _> = [
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
                            shares: BTreeMap::new(),
                            shown_nodes: Vec::new(),
                            shown_shares: Rc::new(RefCell::new(Vec::new())),
                        })
                        .into()
                    }),
                },
            )
        })
        .collect();
        components.insert(
            ComponentId::parse("riso.tabs").unwrap(),
            Component {
                slots: vec![SlotName::parse("children").unwrap()],
                create: Rc::new(|node, slots, _, cx| {
                    let tabs = cx.new(|_| Tabs {
                        node: node.clone(),
                        slots,
                        selected: None,
                    });
                    cx.default_global::<Registry>().tabs.push(tabs.downgrade());
                    tabs.into()
                }),
            },
        );
        Self {
            extensions: BTreeSet::new(),
            components,
            tree: Rc::new(RefCell::new(Tree::default())),
            tabs: Vec::new(),
        }
    }
}
impl Global for Registry {}

struct Split {
    node: NodeId,
    shares: BTreeMap<NodeId, f32>,
    shown_nodes: Vec<NodeId>,
    shown_shares: Rc<RefCell<Vec<f32>>>,
    axis: Axis,
    slots: Slots,
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // The axis mutates this vector during mouse events. Save it against the
        // previous IDs before replacing it with the current shown children.
        for (node, share) in self
            .shown_nodes
            .iter()
            .zip(self.shown_shares.borrow().iter())
        {
            self.shares.insert(node.clone(), *share);
        }
        let nodes = self.slots.nodes("children");
        let shares = nodes
            .iter()
            .map(|node| *self.shares.entry(node.clone()).or_insert(1.))
            .collect();
        if nodes != self.shown_nodes {
            // A changed child set also ends any positional drag in the axis.
            self.shown_shares = Rc::new(RefCell::new(shares));
        } else {
            *self.shown_shares.borrow_mut() = shares;
        }
        self.shown_nodes = nodes;
        riso_ui::dev::axis(
            gpui::SharedString::from(self.node.to_string()),
            self.axis,
            self.shown_shares.clone(),
            self.slots
                .views("children")
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect(),
        )
    }
}

struct Tabs {
    node: NodeId,
    slots: Slots,
    selected: Option<NodeId>,
}

impl Tabs {
    fn reconcile_selection(&mut self) {
        let nodes = self.slots.nodes("children");
        if !self
            .selected
            .as_ref()
            .is_some_and(|node| nodes.contains(node))
        {
            self.selected = nodes.first().cloned();
        }
    }

    fn select(&mut self, node: NodeId, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&node) {
            return;
        }
        self.selected = Some(node.clone());
        cx.notify();
        tracing::debug!(tabs = %self.node, tab = %node, "tab selected");
    }
}

impl Render for Tabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reconcile_selection();
        let nodes = self.slots.nodes("children");
        let selected_index = nodes
            .iter()
            .position(|node| Some(node) == self.selected.as_ref());
        let tabs = nodes
            .into_iter()
            .map(|node| {
                let entity = cx.entity().downgrade();
                let id = gpui::SharedString::from(node.to_string());
                riso_ui::Tab::new(id.clone(), id)
                    .selected(Some(&node) == self.selected.as_ref())
                    .on_click(move |_, cx| {
                        let _ = entity.update(cx, |tabs, cx| tabs.select(node.clone(), cx));
                    })
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(riso_ui::TabBar::new(gpui::SharedString::from(self.node.to_string())).tabs(tabs))
            .children(
                selected_index
                    .and_then(|index| self.slots.views("children").get(index).cloned())
                    .map(|view| div().flex_1().min_h_0().child(view)),
            )
    }
}

/// Returns a node's own visibility flag for extension controls.
pub fn is_hidden(cx: &App, node: &str) -> bool {
    NodeId::parse(node).is_ok_and(|node| {
        cx.try_global::<Registry>()
            .is_some_and(|registry| registry.tree.borrow().is_hidden(&node))
    })
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

    /// Hides or shows any non-root node while retaining its view.
    pub fn set_hidden(&self, cx: &mut App, node: &str, hidden: bool) {
        let _group =
            tracing::info_span!("node-visibility", node, hidden, extension = self.name).entered();
        let node = match NodeId::parse(node) {
            Ok(node) => node,
            Err(error) => {
                log::error!("invalid node ID {node:?}: {error}");
                return;
            }
        };
        let changed = cx
            .default_global::<Registry>()
            .tree
            .borrow_mut()
            .set_hidden(&node, hidden);
        if changed == Ok(true) {
            // Reconcile at the transition, including tabs that cannot currently
            // render under a hidden ancestor or an inactive parent tab.
            let tabs = cx.default_global::<Registry>().tabs.clone();
            for tabs in tabs {
                let _ = tabs.update(cx, |tabs, _| tabs.reconcile_selection());
            }
            cx.refresh_windows();
        }
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
        let _ = cx
            .default_global::<Registry>()
            .tree
            .borrow_mut()
            .place_root(Node {
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
        let _ = cx.default_global::<Registry>().tree.borrow_mut().place(
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
    _views: Vec<(NodeId, AnyView)>,
}

/// Creates the shown nodes once, parent before children, when the window opens.
pub fn new_shell(window: &mut Window, cx: &mut App) -> Entity<Shell> {
    let root = cx
        .default_global::<Registry>()
        .tree
        .borrow()
        .root()
        .cloned();
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
    views: &mut Vec<(NodeId, AnyView)>,
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
            .borrow()
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
    let slots = Slots(
        Rc::new(RefCell::new(Vec::new())),
        cx.default_global::<Registry>().tree.clone(),
    );
    let view = (component.create)(&node.id, slots.clone(), window, cx);
    views.push((node.id.clone(), view.clone()));
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
                .map(|view| (child.id.clone(), view))
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
    if node.component.as_str() == "riso.tabs" {
        tracing::debug!(tabs = slots.views("children").len(), "tabs created");
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
