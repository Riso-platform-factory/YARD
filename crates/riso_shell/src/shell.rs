//! Component factories and the root view kept by the main window.

use gpui::{
    AnyView, App, AppContext, Axis, Bounds, Context, DefiniteLength, DragMoveEvent, Entity,
    EntityId, Global, InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render,
    SharedString, Styled, WeakEntity, Window, canvas, div,
};
use riso_layout::{Applied, ComponentId, Node, NodeId, Operation, SlotName, Tree};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    rc::Rc,
};

type Factory = Rc<dyn Fn(&NodeId, Slots, &mut Window, &mut App) -> AnyView>;
type ViewMap = BTreeMap<NodeId, AnyView>;

#[derive(Clone)]
struct DraggedNode {
    node: NodeId,
    label: SharedString,
}

impl Render for DraggedNode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = riso_ui::palette(cx);
        div()
            .px_3()
            .py_1()
            .bg(palette.background)
            .border_1()
            .border_color(palette.border)
            .text_color(palette.text)
            .font(riso_ui::ui_font())
            .child(self.label.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Before,
    After,
}

#[derive(Clone)]
struct Component {
    slots: Vec<SlotName>,
    create: Factory,
}

/// A node's shown children, read from the tree and view map at call time.
#[derive(Clone)]
pub struct Slots {
    node: NodeId,
    tree: Rc<RefCell<Tree>>,
    views: Rc<RefCell<ViewMap>>,
}

impl Slots {
    pub fn nodes(&self, slot: &str) -> Vec<NodeId> {
        let tree = self.tree.borrow();
        let views = self.views.borrow();
        tree.slots(&self.node)
            .find(|(name, _)| name.as_str() == slot)
            .map(|(_, children)| {
                children
                    .iter()
                    .filter(|child| !tree.is_hidden(&child.id) && views.contains_key(&child.id))
                    .map(|child| child.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn views(&self, slot: &str) -> Vec<AnyView> {
        let tree = self.tree.borrow();
        let views = self.views.borrow();
        tree.slots(&self.node)
            .find(|(name, _)| name.as_str() == slot)
            .map(|(_, children)| {
                children
                    .iter()
                    .filter(|child| !tree.is_hidden(&child.id))
                    .filter_map(|child| views.get(&child.id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn contains_child(&self, slot: &str, child_id: &NodeId) -> bool {
        let tree = self.tree.borrow();
        tree.slots(&self.node)
            .find(|(name, _)| name.as_str() == slot)
            .is_some_and(|(_, children)| children.iter().any(|child| &child.id == child_id))
    }
}

struct Registry {
    extensions: BTreeSet<String>,
    components: BTreeMap<ComponentId, Component>,
    tree: Rc<RefCell<Tree>>,
    views: Rc<RefCell<ViewMap>>,
    splits: Vec<WeakEntity<Split>>,
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
                        let split = cx.new(|_| Split {
                            node: node.clone(),
                            axis,
                            slots,
                            shares: BTreeMap::new(),
                            shown_nodes: Vec::new(),
                            shown_shares: Rc::new(RefCell::new(Vec::new())),
                            edges: BTreeMap::new(),
                            child_bounds: BTreeMap::new(),
                        });
                        cx.default_global::<Registry>()
                            .splits
                            .push(split.downgrade());
                        split.into()
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
                        bar_bounds: None,
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
            views: Rc::new(RefCell::new(BTreeMap::new())),
            splits: Vec::new(),
            tabs: Vec::new(),
        }
    }
}
impl Global for Registry {}

fn edge_at(bounds: Bounds<Pixels>, axis: Axis, position: Point<Pixels>) -> Option<Edge> {
    if !bounds.contains(&position) {
        return None;
    }
    let size = bounds.size.width.min(bounds.size.height) * 0.2;
    let relative = Point::new(position.x - bounds.left(), position.y - bounds.top());
    match axis {
        Axis::Horizontal => {
            if relative.x < size {
                Some(Edge::Before)
            } else if relative.x > bounds.size.width - size {
                Some(Edge::After)
            } else {
                None
            }
        }
        Axis::Vertical => {
            if relative.y < size {
                Some(Edge::Before)
            } else if relative.y > bounds.size.height - size {
                Some(Edge::After)
            } else {
                None
            }
        }
    }
}

fn edge_overlay(axis: Axis, edge: Edge, color: gpui::Hsla) -> gpui::Div {
    let overlay = div()
        .debug_selector(move || {
            let side = match (axis, edge) {
                (Axis::Horizontal, Edge::Before) => "left",
                (Axis::Horizontal, Edge::After) => "right",
                (Axis::Vertical, Edge::Before) => "top",
                (Axis::Vertical, Edge::After) => "bottom",
            };
            format!("DROP-OVERLAY-{side}")
        })
        .absolute()
        .bg(color);
    match (axis, edge) {
        (Axis::Horizontal, Edge::Before) => overlay
            .top_0()
            .left_0()
            .bottom_0()
            .w(DefiniteLength::Fraction(0.5)),
        (Axis::Horizontal, Edge::After) => overlay
            .top_0()
            .right_0()
            .bottom_0()
            .w(DefiniteLength::Fraction(0.5)),
        (Axis::Vertical, Edge::Before) => overlay
            .top_0()
            .left_0()
            .right_0()
            .h(DefiniteLength::Fraction(0.5)),
        (Axis::Vertical, Edge::After) => overlay
            .left_0()
            .right_0()
            .bottom_0()
            .h(DefiniteLength::Fraction(0.5)),
    }
}

fn inner_target_claims_cursor(
    cx: &App,
    current_split: EntityId,
    subtree_root: &NodeId,
    position: Point<Pixels>,
) -> bool {
    let Some(registry) = cx.try_global::<Registry>() else {
        return false;
    };
    let tabs_selected: BTreeMap<NodeId, Option<NodeId>> = registry
        .tabs
        .iter()
        .filter_map(|weak| weak.upgrade())
        .map(|tabs| {
            let tabs = tabs.read(cx);
            (tabs.node.clone(), tabs.selected.clone())
        })
        .collect();
    let mut shown_subtree = BTreeSet::new();
    {
        let tree = registry.tree.borrow();
        let views = registry.views.borrow();
        fn collect(
            tree: &Tree,
            views: &ViewMap,
            tabs_selected: &BTreeMap<NodeId, Option<NodeId>>,
            node: &NodeId,
            out: &mut BTreeSet<NodeId>,
        ) {
            if tree.is_hidden(node) || !views.contains_key(node) || !out.insert(node.clone()) {
                return;
            }
            if let Some(selected) = tabs_selected.get(node) {
                if let Some(child) = selected {
                    collect(tree, views, tabs_selected, child, out);
                }
                return;
            }
            for (_, children) in tree.slots(node) {
                for child in children {
                    collect(tree, views, tabs_selected, &child.id, out);
                }
            }
        }
        collect(
            &tree,
            &views,
            &tabs_selected,
            subtree_root,
            &mut shown_subtree,
        );
    }
    for weak in &registry.tabs {
        if let Some(tabs) = weak.upgrade() {
            let tabs = tabs.read(cx);
            if shown_subtree.contains(&tabs.node)
                && tabs
                    .bar_bounds
                    .is_some_and(|bounds| bounds.contains(&position))
            {
                return true;
            }
        }
    }
    for weak in &registry.splits {
        if let Some(split) = weak.upgrade()
            && split.entity_id() != current_split
        {
            let split = split.read(cx);
            if shown_subtree.contains(&split.node)
                && split.shown_nodes.iter().any(|child| {
                    split
                        .child_bounds
                        .get(child)
                        .is_some_and(|bounds| edge_at(*bounds, split.axis, position).is_some())
                })
            {
                return true;
            }
        }
    }
    false
}

fn clear_drag_edges(cx: &mut App) {
    let splits = cx
        .try_global::<Registry>()
        .map(|registry| registry.splits.clone())
        .unwrap_or_default();
    for split in splits {
        let _ = split.update(cx, |split, cx| {
            if !split.edges.is_empty() {
                split.edges.clear();
                cx.notify();
            }
        });
    }
}

struct Split {
    node: NodeId,
    shares: BTreeMap<NodeId, f32>,
    shown_nodes: Vec<NodeId>,
    shown_shares: Rc<RefCell<Vec<f32>>>,
    edges: BTreeMap<NodeId, Edge>,
    child_bounds: BTreeMap<NodeId, Bounds<Pixels>>,
    axis: Axis,
    slots: Slots,
}

impl Split {
    fn reconcile(&mut self) {
        // The axis mutates this vector during mouse events. Save it against the
        // previous IDs before replacing it with the current shown children.
        for (node, share) in self
            .shown_nodes
            .iter()
            .zip(self.shown_shares.borrow().iter())
        {
            self.shares.insert(node.clone(), *share);
        }
        // A child that left the split keeps no share there; hidden children stay in the slot.
        self.shares
            .retain(|child, _| self.slots.contains_child("children", child));
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
        self.edges.retain(|child, _| nodes.contains(child));
        self.child_bounds.retain(|child, _| nodes.contains(child));
        self.shown_nodes = nodes;
    }

    fn handle_drag_move(
        &mut self,
        child_id: &NodeId,
        event: &DragMoveEvent<DraggedNode>,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position;
        let current_entity = cx.entity().entity_id();
        let next = edge_at(event.bounds, self.axis, position)
            .filter(|_| !inner_target_claims_cursor(cx, current_entity, child_id, position));
        let previous = self.edges.get(child_id).copied();
        if event.bounds.contains(&position) || next != previous {
            tracing::trace!(
                split = %self.node,
                child = %child_id,
                edge = ?next,
                "drag edge"
            );
        }
        if next != previous {
            match next {
                Some(edge) => {
                    self.edges.insert(child_id.clone(), edge);
                }
                None => {
                    self.edges.remove(child_id);
                }
            }
            cx.notify();
        }
    }
}

impl Render for Split {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reconcile();
        if !cx.has_active_drag() {
            self.edges.clear();
        }
        let palette = riso_ui::palette(cx);
        let children = self
            .shown_nodes
            .iter()
            .cloned()
            .zip(self.slots.views("children"))
            .enumerate()
            .map(|(shown_index, (child_id, view))| {
                let edge = self.edges.get(&child_id).copied();
                let move_child = child_id.clone();
                let bounds_child = child_id.clone();
                let bounds_entity = cx.entity().downgrade();
                let split_node = self.node.clone();
                let zone_id = SharedString::from(format!("{}:{child_id}", self.node));
                let mut zone = div()
                    .id(zone_id)
                    .relative()
                    .size_full()
                    .on_drag_move::<DraggedNode>(cx.listener(move |split, event, _, cx| {
                        split.handle_drag_move(&move_child, event, cx);
                    }))
                    .child(view)
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                let _ = bounds_entity.update(cx, |split, _| {
                                    split.child_bounds.insert(bounds_child.clone(), bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    );
                if let Some(edge) = edge {
                    zone = zone
                        .on_drop::<DraggedNode>(move |dragged, _, cx| {
                            drop_on_edge(cx, &dragged.node, &split_node, shown_index, edge);
                        })
                        .child(edge_overlay(self.axis, edge, palette.drop_target));
                }
                zone.into_any_element()
            })
            .collect();
        riso_ui::axis(
            SharedString::from(self.node.to_string()),
            self.axis,
            self.shown_shares.clone(),
            children,
        )
    }
}

struct Tabs {
    node: NodeId,
    slots: Slots,
    selected: Option<NodeId>,
    bar_bounds: Option<Bounds<Pixels>>,
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
        let parent = self.node.clone();
        let tabs = nodes
            .iter()
            .map(|node| {
                let entity = cx.entity().downgrade();
                let click_node = node.clone();
                let target_node = node.clone();
                let target_parent = parent.clone();
                let label = SharedString::from(node.to_string());
                let dragged = DraggedNode {
                    node: node.clone(),
                    label: label.clone(),
                };
                riso_ui::Tab::new(label.clone(), label)
                    .selected(Some(node) == self.selected.as_ref())
                    .on_click(move |_, cx| {
                        let _ = entity.update(cx, |tabs, cx| tabs.select(click_node.clone(), cx));
                    })
                    .on_drag(dragged, |dragged, _, cx| {
                        tracing::debug!(node = %dragged.node, "drag started");
                        cx.new(|_| dragged.clone()).into()
                    })
                    .on_drop::<DraggedNode>(move |dragged, _, cx| {
                        drop_on_tab(cx, &dragged.node, &target_parent, &target_node);
                    })
            })
            .collect();
        let bar_parent = self.node.clone();
        let bar_entity = cx.entity().downgrade();
        let bar = riso_ui::TabBar::new(SharedString::from(self.node.to_string()))
            .tabs(tabs)
            .on_drop::<DraggedNode>(move |dragged, _, cx| {
                drop_on_tab_bar(cx, &dragged.node, &bar_parent);
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .on_children_prepainted(move |children_bounds, _, cx| {
                if let Some(&bounds) = children_bounds.first() {
                    let _ = bar_entity.update(cx, |tabs, _| {
                        tabs.bar_bounds = Some(bounds);
                    });
                }
            })
            .child(bar)
            .children(
                selected_index
                    .and_then(|index| self.slots.views("children").get(index).cloned())
                    .map(|view| div().flex_1().min_h_0().child(view)),
            )
    }
}

fn current_location(tree: &Tree, target: &NodeId) -> Option<(NodeId, SlotName, usize, usize)> {
    fn find(
        tree: &Tree,
        parent: &NodeId,
        target: &NodeId,
    ) -> Option<(NodeId, SlotName, usize, usize)> {
        for (slot, children) in tree.slots(parent) {
            if let Some(index) = children.iter().position(|child| &child.id == target) {
                return Some((parent.clone(), slot.clone(), index, children.len()));
            }
            for child in children {
                if let Some(found) = find(tree, &child.id, target) {
                    return Some(found);
                }
            }
        }
        None
    }
    find(tree, &tree.root()?.id, target)
}

fn slot_index_of(tree: &Tree, parent: &NodeId, slot: &str, child_id: &NodeId) -> Option<usize> {
    tree.slots(parent)
        .find(|(name, _)| name.as_str() == slot)
        .and_then(|(_, children)| children.iter().position(|child| &child.id == child_id))
}

fn is_own_position(tree: &Tree, node: &NodeId, parent: &NodeId, slot: &str, index: usize) -> bool {
    current_location(tree, node).is_some_and(|(cur_parent, cur_slot, cur_index, cur_len)| {
        &cur_parent == parent && cur_slot.as_str() == slot && index.min(cur_len - 1) == cur_index
    })
}

fn drop_on_tab(cx: &mut App, node: &NodeId, parent: &NodeId, target_tab: &NodeId) {
    clear_drag_edges(cx);
    let index = {
        let tree = cx.default_global::<Registry>().tree.borrow();
        let Some(index) = slot_index_of(&tree, parent, "children", target_tab) else {
            return;
        };
        if is_own_position(&tree, node, parent, "children", index) {
            tracing::debug!(node = %node, "drop ignored");
            return;
        }
        index
    };
    shell_move(cx, node, parent, "children", index);
}

fn drop_on_tab_bar(cx: &mut App, node: &NodeId, parent: &NodeId) {
    clear_drag_edges(cx);
    {
        let tree = cx.default_global::<Registry>().tree.borrow();
        if is_own_position(&tree, node, parent, "children", usize::MAX) {
            tracing::debug!(node = %node, "drop ignored");
            return;
        }
    }
    shell_move(cx, node, parent, "children", usize::MAX);
}

fn drop_on_edge(cx: &mut App, node: &NodeId, split_id: &NodeId, shown_index: usize, edge: Edge) {
    clear_drag_edges(cx);
    let shown_child = cx.try_global::<Registry>().and_then(|registry| {
        registry
            .splits
            .iter()
            .filter_map(|weak| weak.upgrade())
            .find(|split| &split.read(cx).node == split_id)
            .and_then(|split| split.read(cx).shown_nodes.get(shown_index).cloned())
    });
    let Some(shown_child) = shown_child else {
        return;
    };
    let index = {
        let tree = cx.default_global::<Registry>().tree.borrow();
        let Some(slot_pos) = slot_index_of(&tree, split_id, "children", &shown_child) else {
            return;
        };
        let gap = match edge {
            Edge::Before => slot_pos,
            Edge::After => slot_pos + 1,
        };
        let index = match current_location(&tree, node) {
            Some((cur_parent, cur_slot, cur_index, _))
                if &cur_parent == split_id
                    && cur_slot.as_str() == "children"
                    && cur_index < gap =>
            {
                gap - 1
            }
            _ => gap,
        };
        if is_own_position(&tree, node, split_id, "children", index) {
            tracing::debug!(node = %node, "drop ignored");
            return;
        }
        index
    };
    shell_move(cx, node, split_id, "children", index);
}

fn shell_move(cx: &mut App, node: &NodeId, parent: &NodeId, slot: &str, index: usize) {
    run_op("riso", None, cx, "move", node.as_str(), |_, node| {
        let slot = SlotName::parse(slot)
            .map_err(|error| format!("invalid slot name {slot:?}: {error}"))?;
        Ok(Operation::Move {
            node,
            parent: parent.clone(),
            slot,
            index,
        })
    });
}

/// Returns a node's own visibility flag for extension controls.
pub fn is_hidden(cx: &App, node: &str) -> bool {
    NodeId::parse(node).is_ok_and(|node| {
        cx.try_global::<Registry>()
            .is_some_and(|registry| registry.tree.borrow().is_hidden(&node))
    })
}

fn component_of<'a>(tree: &'a Tree, id: &NodeId) -> Option<&'a ComponentId> {
    fn find<'a>(tree: &'a Tree, node: &'a Node, id: &NodeId) -> Option<&'a ComponentId> {
        if &node.id == id {
            return Some(&node.component);
        }
        tree.slots(&node.id)
            .flat_map(|(_, children)| children)
            .find_map(|child| find(tree, child, id))
    }
    find(tree, tree.root()?, id)
}

fn reconcile_containers(cx: &mut App) {
    let (splits, tabs) = {
        let registry = cx.default_global::<Registry>();
        registry.splits.retain(|split| split.upgrade().is_some());
        registry.tabs.retain(|tabs| tabs.upgrade().is_some());
        (registry.splits.clone(), registry.tabs.clone())
    };
    for split in splits {
        let _ = split.update(cx, |split, _| split.reconcile());
    }
    for tabs in tabs {
        let _ = tabs.update(cx, |tabs, _| tabs.reconcile_selection());
    }
}

fn run_op(
    extension: &str,
    window: Option<&mut Window>,
    cx: &mut App,
    op: &'static str,
    node: &str,
    build: impl FnOnce(String, NodeId) -> Result<Operation, String>,
) {
    let _group = tracing::info_span!("layout-op", op, node, extension).entered();
    let node_id = match NodeId::parse(node) {
        Ok(id) => id,
        Err(error) => {
            log::error!("invalid node ID {node:?}: {error}");
            return;
        }
    };
    let operation = match build(extension.to_string(), node_id) {
        Ok(operation) => operation,
        Err(message) => {
            log::error!("{message}");
            return;
        }
    };
    if !check_shell_rules(extension, cx, &operation) {
        return;
    }
    let added = match &operation {
        Operation::Add { node, .. } => Some(node.clone()),
        _ => None,
    };
    let applied = {
        let tree = cx.default_global::<Registry>().tree.clone();
        let mut tree = tree.borrow_mut();
        tree.apply(operation)
    };
    let Ok(applied) = applied else {
        return;
    };
    match applied {
        Applied::Added => {
            if let (Some(node), Some(window)) = (added, window) {
                let _ = create_view(&node, None, window, cx);
            }
        }
        Applied::Removed(removed) => {
            for id in removed {
                let dropped = cx
                    .default_global::<Registry>()
                    .views
                    .borrow_mut()
                    .remove(&id);
                if dropped.is_some() {
                    drop(dropped);
                    tracing::debug!(node = %id, "view dropped");
                }
            }
        }
        Applied::Moved | Applied::SetHidden(true) => {}
        Applied::SetHidden(false) => return,
    }
    reconcile_containers(cx);
    cx.refresh_windows();
}

fn check_shell_rules(extension: &str, cx: &mut App, operation: &Operation) -> bool {
    let registry = cx.default_global::<Registry>();
    match operation {
        Operation::Add { node, parent, slot } => {
            let namespace = node.component.as_str().split('.').next();
            if namespace != Some(extension) && namespace != Some("riso") {
                tracing::info!(
                    node = %node.id,
                    component = %node.component,
                    extension = %extension,
                    "component is not in the extension's namespace",
                );
                return false;
            }
            if !registry.components.contains_key(&node.component) {
                tracing::info!(
                    node = %node.id,
                    component = %node.component,
                    "component is not registered",
                );
                return false;
            }
            let tree = registry.tree.borrow();
            if let Some(parent_component) = component_of(&tree, parent)
                && !registry
                    .components
                    .get(parent_component)
                    .is_some_and(|component| component.slots.contains(slot))
            {
                tracing::info!(
                    node = %node.id,
                    parent = %parent,
                    parent_component = %parent_component,
                    slot = %slot,
                    "parent component does not declare slot",
                );
                return false;
            }
            true
        }
        Operation::Move {
            node, parent, slot, ..
        } => {
            let tree = registry.tree.borrow();
            if let Some(parent_component) = component_of(&tree, parent)
                && !registry
                    .components
                    .get(parent_component)
                    .is_some_and(|component| component.slots.contains(slot))
            {
                tracing::info!(
                    node = %node,
                    parent = %parent,
                    parent_component = %parent_component,
                    slot = %slot,
                    "parent component does not declare slot",
                );
                return false;
            }
            true
        }
        Operation::Remove { .. } | Operation::SetHidden { .. } => true,
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

    /// Adds a new node at the end of a parent's declared slot and creates its view.
    pub fn add(
        &self,
        window: &mut Window,
        cx: &mut App,
        node: &str,
        component: &str,
        parent: &str,
        slot: &str,
    ) {
        run_op(
            &self.name,
            Some(window),
            cx,
            "add",
            node,
            |extension, id| {
                let component = ComponentId::parse(component)
                    .map_err(|error| format!("invalid component ID {component:?}: {error}"))?;
                let parent = NodeId::parse(parent)
                    .map_err(|error| format!("invalid parent ID {parent:?}: {error}"))?;
                let slot = SlotName::parse(slot)
                    .map_err(|error| format!("invalid slot name {slot:?}: {error}"))?;
                Ok(Operation::Add {
                    node: Node {
                        id,
                        component,
                        extension,
                    },
                    parent,
                    slot,
                })
            },
        );
    }

    /// Removes a non-root node and its subtree, dropping their views.
    pub fn remove(&self, cx: &mut App, node: &str) {
        run_op(&self.name, None, cx, "remove", node, |_, node| {
            Ok(Operation::Remove { node })
        });
    }

    /// Moves a non-root node with its subtree into a declared slot at `index`.
    pub fn move_node(&self, cx: &mut App, node: &str, parent: &str, slot: &str, index: usize) {
        run_op(&self.name, None, cx, "move", node, |_, node| {
            let parent = NodeId::parse(parent)
                .map_err(|error| format!("invalid parent ID {parent:?}: {error}"))?;
            let slot = SlotName::parse(slot)
                .map_err(|error| format!("invalid slot name {slot:?}: {error}"))?;
            Ok(Operation::Move {
                node,
                parent,
                slot,
                index,
            })
        });
    }

    /// Hides or shows any non-root node while retaining its view.
    pub fn set_hidden(&self, cx: &mut App, node: &str, hidden: bool) {
        let op = if hidden { "hide" } else { "show" };
        run_op(&self.name, None, cx, op, node, |_, node| {
            Ok(Operation::SetHidden { node, hidden })
        });
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

/// The window's root view. All views live in the shell's global map.
pub struct Shell {
    root: Option<AnyView>,
}

/// Creates the initial nodes once, parent before children, when the window opens.
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
    let root = root.and_then(|node| create_view(&node, None, window, cx));
    cx.new(|_| Shell { root })
}

fn create_view(
    node: &Node,
    parent: Option<(&ComponentId, &[SlotName], &SlotName)>,
    window: &mut Window,
    cx: &mut App,
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
    let (component, children, slots) = {
        let registry = cx.default_global::<Registry>();
        let component = registry.components.get(&node.component).cloned();
        let children = registry
            .tree
            .borrow()
            .slots(&node.id)
            .map(|(slot, nodes)| (slot.clone(), nodes.to_vec()))
            .collect::<Vec<_>>();
        let slots = Slots {
            node: node.id.clone(),
            tree: registry.tree.clone(),
            views: registry.views.clone(),
        };
        (component, children, slots)
    };
    let Some(component) = component else {
        log::error!(
            "node {} names unregistered component {}",
            node.id,
            node.component
        );
        return None;
    };
    let view = (component.create)(&node.id, slots.clone(), window, cx);
    cx.default_global::<Registry>()
        .views
        .borrow_mut()
        .insert(node.id.clone(), view.clone());
    for (slot, nodes) in children {
        for child in &nodes {
            let _ = create_view(
                child,
                Some((&node.component, &component.slots, &slot)),
                window,
                cx,
            );
        }
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
            .id("shell")
            .size_full()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .bg(riso_ui::palette(cx).background)
            .font(riso_ui::ui_font())
            .on_drop::<DraggedNode>(|dragged, _, cx| {
                clear_drag_edges(cx);
                tracing::debug!(node = %dragged.node, "drop ignored");
            })
            .children(self.root.clone())
    }
}
