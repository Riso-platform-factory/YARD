//! Component factories and the root view kept by the main window.

gpui::actions!(
    riso,
    [
        ToggleZoom,
        Hide,
        SelectNextTab,
        SelectPreviousTab,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown
    ]
);

gpui::actions!(riso, [Dismiss]);
gpui::actions!(riso, [Quit]);
use gpui::{
    Action, AnyView, App, AppContext, Axis, Bounds, Context, DefiniteLength, Div, DragMoveEvent,
    Entity, EntityId, FocusHandle, Global, InteractiveElement, IntoElement, KeyBinding,
    KeyBindingContextPredicate, ParentElement, Pixels, Point, Render, SharedString, Styled,
    Subscription, WeakEntity, Window, canvas, div,
};
use riso_layout::Layers;
use riso_layout::{Applied, ComponentId, Node, NodeId, Operation, Position, Props, SlotName, Tree};
use riso_layout::{decode, encode};
use riso_paths::write_atomic;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    path::PathBuf,
    rc::Rc,
};

/// A component view that can be told updated props.
pub trait PropsView<P>: Render {
    fn set_props(&mut self, props: P, cx: &mut Context<Self>);
}

/// A component view that supplies its tab title and optional icon.
pub trait Titled: Render {
    fn title(&self, cx: &App) -> SharedString;
    fn icon(&self, _cx: &App) -> Option<riso_ui::IconName> {
        None
    }
}

type TitleReader = Rc<dyn Fn(&AnyView, &App) -> Option<(SharedString, Option<riso_ui::IconName>)>>;

type ActionHandler = Rc<dyn Fn(Div) -> Div>;
type Factory = Rc<dyn Fn(&Node, Slots, &mut Window, &mut App) -> AnyView>;
type Teller = Rc<dyn Fn(&Props, &mut App)>;
type ViewMap = BTreeMap<NodeId, AnyView>;

const FRAME_NODES: &[&str] = &["window", "title_bar", "body", "status_bar"];
const FRAME_COMPONENTS: &[&str] = &[
    "riso.window",
    "riso.title_bar",
    "riso.body",
    "riso.status_bar",
];

fn is_frame_node(id: &NodeId) -> bool {
    FRAME_NODES.contains(&id.as_str())
}

fn is_frame_component(id: &ComponentId) -> bool {
    FRAME_COMPONENTS.contains(&id.as_str())
}

fn is_frame_slot(parent: &NodeId, slot: &SlotName) -> bool {
    matches!(
        (parent.as_str(), slot.as_str()),
        ("window", "title_bar" | "body" | "status_bar") | ("body", "children")
    )
}

fn is_bar_item(tree: &Tree, id: &NodeId) -> bool {
    current_location(tree, id)
        .is_some_and(|(parent, _, _, _)| matches!(parent.as_str(), "title_bar" | "status_bar"))
}

fn is_in_body(tree: &Tree, id: &NodeId) -> bool {
    let mut current = id.clone();
    while let Some((parent, _, _, _)) = current_location(tree, &current) {
        if parent.as_str() == "body" {
            return true;
        }
        current = parent;
    }
    false
}

struct WindowFrame {
    slots: Slots,
}

impl Render for WindowFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        riso_ui::v_flex()
            .size_full()
            .children(self.slots.views("title_bar"))
            .children(self.slots.views("body").into_iter().map(|view| {
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .grid()
                    .grid_cols(1)
                    .grid_rows(1)
                    .child(view)
            }))
            .children(self.slots.views("status_bar"))
    }
}

struct TitleBarFrame {
    node: NodeId,
    slots: Slots,
}

impl Render for TitleBarFrame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left = self.slots.views("left");
        let center = self.slots.views("center");
        let right = self.slots.views("right");
        let title_shown = left.is_empty();
        tracing::trace!(
            left = left.len(),
            center = center.len(),
            right = right.len(),
            title = title_shown,
            "rendering title bar",
        );
        let window_title = cx
            .try_global::<Registry>()
            .map(|registry| registry.window_title.clone())
            .unwrap_or_default();
        let mut bar = riso_ui::TitleBar::new(SharedString::from(self.node.to_string()));
        if title_shown {
            let selector_title = window_title.clone();
            bar = bar.left(
                div()
                    .debug_selector(move || format!("WINDOW-TITLE-{selector_title}"))
                    .child(
                        riso_ui::Label::new(window_title)
                            .size(riso_ui::LabelSize::Small)
                            .color(riso_ui::TextColor::Muted),
                    ),
            );
        } else {
            for view in left {
                bar = bar.left(view);
            }
        }
        for view in center {
            bar = bar.center(view);
        }
        for view in right {
            bar = bar.right(view);
        }
        bar
    }
}

struct StatusBarFrame {
    node: NodeId,
    slots: Slots,
}

impl Render for StatusBarFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let left = self.slots.views("left");
        let right = self.slots.views("right");
        if left.is_empty() && right.is_empty() {
            return div().into_any_element();
        }
        tracing::trace!(
            left = left.len(),
            right = right.len(),
            "rendering status bar"
        );
        let mut bar = riso_ui::StatusBar::new(SharedString::from(self.node.to_string()));
        for view in left {
            bar = bar.left(view);
        }
        for view in right {
            bar = bar.right(view);
        }
        bar.into_any_element()
    }
}

struct BodyFrame {
    slots: Slots,
}

impl Render for BodyFrame {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zoomed_view = cx.try_global::<Registry>().and_then(|registry| {
            registry
                .zoomed
                .as_ref()
                .and_then(|node| registry.views.borrow().get(node).cloned())
        });
        let views = match zoomed_view {
            Some(view) => vec![view],
            None => self.slots.views("children"),
        };
        div()
            .size_full()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .children(views)
    }
}

struct NodeView {
    node: NodeId,
    handle: Option<FocusHandle>,
    view: AnyView,
}

impl Render for NodeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let node = self.node.clone();
        let in_bar = cx.try_global::<Registry>().is_some_and(|registry| {
            let layers = registry.tree.borrow();
            is_bar_item(layers.current(), &self.node)
        });
        let container = if matches!(self.node.as_str(), "title_bar" | "status_bar") {
            div().relative().w_full().flex_none()
        } else if in_bar {
            riso_ui::h_flex().relative().h_full().flex_shrink_0()
        } else {
            div()
                .relative()
                .size_full()
                .grid()
                .grid_cols(1)
                .grid_rows(1)
        };
        let container = if let Some(handle) = &self.handle {
            container
                .track_focus(handle)
                .on_action(cx.listener(|view, _: &ToggleZoom, window, cx| {
                    layout_action(LayoutAction::ToggleZoom, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &Hide, window, cx| {
                    layout_action(LayoutAction::Hide, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &SelectNextTab, window, cx| {
                    layout_action(LayoutAction::SelectNextTab, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &FocusLeft, window, cx| {
                    layout_action(LayoutAction::FocusLeft, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &FocusRight, window, cx| {
                    layout_action(LayoutAction::FocusRight, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &FocusUp, window, cx| {
                    layout_action(LayoutAction::FocusUp, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &FocusDown, window, cx| {
                    layout_action(LayoutAction::FocusDown, Some(&view.node), window, cx);
                }))
                .on_action(cx.listener(|view, _: &SelectPreviousTab, window, cx| {
                    layout_action(
                        LayoutAction::SelectPreviousTab,
                        Some(&view.node),
                        window,
                        cx,
                    );
                }))
        } else {
            container
        };
        container.child(self.view.clone()).child(
            canvas(
                move |bounds, _, cx| {
                    cx.default_global::<Registry>()
                        .bounds
                        .insert(node.clone(), bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

#[derive(Clone)]
struct DraggedNode {
    node: NodeId,
    label: SharedString,
    icon: Option<riso_ui::IconName>,
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
            .font(riso_ui::ui_font(cx))
            .child(
                riso_ui::h_flex()
                    .gap_1()
                    .children(self.icon.map(|icon| {
                        riso_ui::Icon::new(icon)
                            .size(riso_ui::IconSize::Small)
                            .color(riso_ui::TextColor::Muted)
                    }))
                    .child(riso_ui::Label::new(self.label.clone())),
            )
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
    tree: Rc<RefCell<Layers>>,
    views: Rc<RefCell<ViewMap>>,
}

impl Slots {
    pub fn nodes(&self, slot: &str) -> Vec<NodeId> {
        let layers = self.tree.borrow();
        let tree = layers.current();
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
        let layers = self.tree.borrow();
        let tree = layers.current();
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
        let layers = self.tree.borrow();
        let tree = layers.current();
        tree.slots(&self.node)
            .find(|(name, _)| name.as_str() == slot)
            .is_some_and(|(_, children)| children.iter().any(|child| &child.id == child_id))
    }
}

struct Registry {
    extensions: BTreeSet<String>,
    components: BTreeMap<ComponentId, Component>,
    action_handlers: Vec<ActionHandler>,
    titles: BTreeMap<ComponentId, TitleReader>,
    required: BTreeSet<ComponentId>,
    slots_for_one: Vec<(ComponentId, SlotName)>,
    title_errors: RefCell<BTreeSet<NodeId>>,
    tree: Rc<RefCell<Layers>>,
    views: Rc<RefCell<ViewMap>>,
    tellers: BTreeMap<NodeId, Teller>,
    focus_handles: BTreeMap<NodeId, FocusHandle>,
    bounds: BTreeMap<NodeId, Bounds<Pixels>>,
    root_focus_handle: Option<FocusHandle>,
    splits: Vec<WeakEntity<Split>>,
    tabs: Vec<WeakEntity<Tabs>>,
    extension_root: Option<NodeId>,
    zoomed: Option<NodeId>,
    window_title: SharedString,
    layout_file: Option<PathBuf>,
    views_created: bool,
    default_fixed: bool,
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
                            node: node.id.clone(),
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
                        node: node.id.clone(),
                        slots,
                        selected: None,
                        bar_bounds: None,
                    });
                    cx.default_global::<Registry>().tabs.push(tabs.downgrade());
                    tabs.into()
                }),
            },
        );
        components.insert(
            ComponentId::parse("riso.window").unwrap(),
            Component {
                slots: vec![
                    SlotName::parse("title_bar").unwrap(),
                    SlotName::parse("body").unwrap(),
                    SlotName::parse("status_bar").unwrap(),
                ],
                create: Rc::new(|_, slots, _, cx| cx.new(|_| WindowFrame { slots }).into()),
            },
        );
        components.insert(
            ComponentId::parse("riso.title_bar").unwrap(),
            Component {
                slots: vec![
                    SlotName::parse("left").unwrap(),
                    SlotName::parse("center").unwrap(),
                    SlotName::parse("right").unwrap(),
                ],
                create: Rc::new(|node, slots, _, cx| {
                    cx.new(|_| TitleBarFrame {
                        node: node.id.clone(),
                        slots,
                    })
                    .into()
                }),
            },
        );
        components.insert(
            ComponentId::parse("riso.body").unwrap(),
            Component {
                slots: vec![SlotName::parse("children").unwrap()],
                create: Rc::new(|_, slots, _, cx| cx.new(|_| BodyFrame { slots }).into()),
            },
        );
        components.insert(
            ComponentId::parse("riso.status_bar").unwrap(),
            Component {
                slots: vec![
                    SlotName::parse("left").unwrap(),
                    SlotName::parse("right").unwrap(),
                ],
                create: Rc::new(|node, slots, _, cx| {
                    cx.new(|_| StatusBarFrame {
                        node: node.id.clone(),
                        slots,
                    })
                    .into()
                }),
            },
        );
        let mut tree = Tree::default();
        {
            let _frame = tracing::info_span!("window-frame").entered();
            let window_id = NodeId::parse("window").unwrap();
            {
                let _place =
                    tracing::info_span!("root-place", node = "window", component = "riso.window",)
                        .entered();
                let _ = tree.place_root(Node {
                    id: window_id.clone(),
                    component: ComponentId::parse("riso.window").unwrap(),
                    extension: "riso".into(),
                    props: Props::new(),
                });
            }
            {
                let _place = tracing::info_span!(
                    "node-place",
                    node = "title_bar",
                    component = "riso.title_bar",
                    parent = "window",
                    slot = "title_bar",
                )
                .entered();
                let _ = tree.place(
                    Node {
                        id: NodeId::parse("title_bar").unwrap(),
                        component: ComponentId::parse("riso.title_bar").unwrap(),
                        extension: "riso".into(),
                        props: Props::new(),
                    },
                    &window_id,
                    SlotName::parse("title_bar").unwrap(),
                );
            }
            {
                let _place = tracing::info_span!(
                    "node-place",
                    node = "body",
                    component = "riso.body",
                    parent = "window",
                    slot = "body",
                )
                .entered();
                let _ = tree.place(
                    Node {
                        id: NodeId::parse("body").unwrap(),
                        component: ComponentId::parse("riso.body").unwrap(),
                        extension: "riso".into(),
                        props: Props::new(),
                    },
                    &window_id,
                    SlotName::parse("body").unwrap(),
                );
            }
            {
                let _place = tracing::info_span!(
                    "node-place",
                    node = "status_bar",
                    component = "riso.status_bar",
                    parent = "window",
                    slot = "status_bar"
                )
                .entered();
                let _ = tree.place(
                    Node {
                        id: NodeId::parse("status_bar").unwrap(),
                        component: ComponentId::parse("riso.status_bar").unwrap(),
                        extension: "riso".into(),
                        props: Props::new(),
                    },
                    &window_id,
                    SlotName::parse("status_bar").unwrap(),
                );
            }
        }
        Self {
            extensions: BTreeSet::new(),
            components,
            action_handlers: Vec::new(),
            titles: BTreeMap::new(),
            required: BTreeSet::new(),
            slots_for_one: Vec::new(),
            title_errors: RefCell::new(BTreeSet::new()),
            tree: Rc::new(RefCell::new(Layers::new(tree))),
            views: Rc::new(RefCell::new(BTreeMap::new())),
            tellers: BTreeMap::new(),
            focus_handles: BTreeMap::new(),
            bounds: BTreeMap::new(),
            root_focus_handle: None,
            splits: Vec::new(),
            tabs: Vec::new(),
            extension_root: None,
            zoomed: None,
            window_title: SharedString::default(),
            layout_file: None,
            views_created: false,
            default_fixed: false,
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
        let layers = registry.tree.borrow();
        let tree = layers.current();
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
            tree,
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
                let (label, icon) = tab_title(node, cx);
                let dragged = DraggedNode {
                    node: node.clone(),
                    label: label.clone(),
                    icon,
                };
                let tab = riso_ui::Tab::new(SharedString::from(node.to_string()), label);
                let tab = if let Some(icon) = icon {
                    tab.icon(icon)
                } else {
                    tab
                };
                tab.selected(Some(node) == self.selected.as_ref())
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

fn tab_title(node: &NodeId, cx: &App) -> (SharedString, Option<riso_ui::IconName>) {
    let fallback = || (SharedString::from(node.to_string()), None);
    let registry = cx.global::<Registry>();
    let layers = registry.tree.borrow();
    let Some(component) = node_of(layers.current(), node).map(|node| &node.component) else {
        return fallback();
    };
    let Some(reader) = registry.titles.get(component) else {
        return fallback();
    };
    let views = registry.views.borrow();
    let Some(view) = views.get(node) else {
        return fallback();
    };
    let wrapped = view
        .clone()
        .downcast::<NodeView>()
        .expect("the registry stores node views");
    match reader(&wrapped.read(cx).view, cx) {
        Some((title, icon)) => {
            tracing::trace!(node = %node, title = %title, "tab title");
            (title, icon)
        }
        None => {
            if registry.title_errors.borrow_mut().insert(node.clone()) {
                log::error!("view of node {node} is not the type that gives its title");
            }
            tracing::trace!("view of node {node} is not the type that gives its title");
            fallback()
        }
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
        let layers = cx.default_global::<Registry>().tree.borrow();
        let tree = layers.current();
        let Some(index) = slot_index_of(tree, parent, "children", target_tab) else {
            return;
        };
        if is_own_position(tree, node, parent, "children", index) {
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
        let layers = cx.default_global::<Registry>().tree.borrow();
        let tree = layers.current();
        if is_own_position(tree, node, parent, "children", usize::MAX) {
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
        let layers = cx.default_global::<Registry>().tree.borrow();
        let tree = layers.current();
        let Some(slot_pos) = slot_index_of(tree, split_id, "children", &shown_child) else {
            return;
        };
        let gap = match edge {
            Edge::Before => slot_pos,
            Edge::After => slot_pos + 1,
        };
        let index = match current_location(tree, node) {
            Some((cur_parent, cur_slot, cur_index, _))
                if &cur_parent == split_id
                    && cur_slot.as_str() == "children"
                    && cur_index < gap =>
            {
                gap - 1
            }
            _ => gap,
        };
        if is_own_position(tree, node, split_id, "children", index) {
            tracing::debug!(node = %node, "drop ignored");
            return;
        }
        index
    };
    shell_move(cx, node, split_id, "children", index);
}

/// A position as a log field: an index logs as its number.
fn index_field(at: &Position) -> String {
    match at {
        Position::Last => "last".into(),
        Position::First => "first".into(),
        Position::Index(index) => index.to_string(),
        Position::Before(node) => format!("before {node}"),
        Position::After(node) => format!("after {node}"),
    }
}

fn shell_move(cx: &mut App, node: &NodeId, parent: &NodeId, slot: &str, index: usize) {
    run_op("riso", None, cx, "move", node.as_str(), |_, node| {
        let slot = SlotName::parse(slot)
            .map_err(|error| format!("invalid slot name {slot:?}: {error}"))?;
        Ok(Operation::Move {
            node,
            parent: parent.clone(),
            slot,
            at: Position::Index(index),
        })
    });
}

/// Returns a node's own visibility flag for extension controls.
pub fn is_hidden(cx: &App, node: &str) -> bool {
    NodeId::parse(node).is_ok_and(|node| {
        cx.try_global::<Registry>()
            .is_some_and(|registry| registry.tree.borrow().current().is_hidden(&node))
    })
}

/// Returns the number of recorded runtime layout operations.
pub fn user_layer_len(cx: &App) -> usize {
    cx.try_global::<Registry>()
        .map_or(0, |registry| registry.tree.borrow().user_layer().len())
}

/// Sets the file where the user layer is saved and from which it is restored.
pub fn set_layout_file(cx: &mut App, path: PathBuf) {
    tracing::debug!(path = %path.display(), "layout file set");
    cx.default_global::<Registry>().layout_file = Some(path);
}

/// Sets the title shown in the title bar when its `left` slot has no shown item.
pub fn set_window_title(cx: &mut App, title: impl Into<SharedString>) {
    let title = title.into();
    let registry = cx.default_global::<Registry>();
    tracing::debug!(title = %title, "window title");
    registry.window_title = title;
    cx.refresh_windows();
}

/// Binds the shell's default keys, including the application-wide quit key.
pub fn bind_default_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    const DEFAULT_KEYS: &[(&str, LayoutAction)] = &[
        ("shift-escape", LayoutAction::ToggleZoom),
        ("cmd-}", LayoutAction::SelectNextTab),
        ("alt-cmd-right", LayoutAction::SelectNextTab),
        ("cmd-{", LayoutAction::SelectPreviousTab),
        ("alt-cmd-left", LayoutAction::SelectPreviousTab),
        ("cmd-k cmd-left", LayoutAction::FocusLeft),
        ("cmd-k cmd-right", LayoutAction::FocusRight),
        ("cmd-k cmd-up", LayoutAction::FocusUp),
        ("cmd-k cmd-down", LayoutAction::FocusDown),
    ];
    #[cfg(not(target_os = "macos"))]
    const DEFAULT_KEYS: &[(&str, LayoutAction)] = &[
        ("shift-escape", LayoutAction::ToggleZoom),
        ("ctrl-pagedown", LayoutAction::SelectNextTab),
        ("ctrl-pageup", LayoutAction::SelectPreviousTab),
        ("ctrl-k ctrl-left", LayoutAction::FocusLeft),
        ("ctrl-k ctrl-right", LayoutAction::FocusRight),
        ("ctrl-k ctrl-up", LayoutAction::FocusUp),
        ("ctrl-k ctrl-down", LayoutAction::FocusDown),
    ];
    let count = DEFAULT_KEYS.len() + 2;
    let _group = tracing::info_span!("keys-bind", count).entered();
    let mut logged = Vec::with_capacity(count);
    let mut bindings: Vec<KeyBinding> = DEFAULT_KEYS
        .iter()
        .map(|&(keys, action)| {
            let binding = match action {
                LayoutAction::ToggleZoom => KeyBinding::new(keys, ToggleZoom, Some("Shell")),
                LayoutAction::Hide => KeyBinding::new(keys, Hide, Some("Shell")),
                LayoutAction::FocusLeft => KeyBinding::new(keys, FocusLeft, Some("Shell")),
                LayoutAction::FocusRight => KeyBinding::new(keys, FocusRight, Some("Shell")),
                LayoutAction::FocusUp => KeyBinding::new(keys, FocusUp, Some("Shell")),
                LayoutAction::FocusDown => KeyBinding::new(keys, FocusDown, Some("Shell")),
                LayoutAction::SelectNextTab => KeyBinding::new(keys, SelectNextTab, Some("Shell")),
                LayoutAction::SelectPreviousTab => {
                    KeyBinding::new(keys, SelectPreviousTab, Some("Shell"))
                }
            };
            logged.push((keys, binding.action().name(), "Shell"));
            binding
        })
        .collect();
    let dismiss = KeyBinding::new("escape", Dismiss, Some("ModalLayer"));
    logged.push(("escape", dismiss.action().name(), "ModalLayer"));
    bindings.push(dismiss);
    #[cfg(target_os = "macos")]
    let quit_key = "cmd-q";
    #[cfg(not(target_os = "macos"))]
    let quit_key = "ctrl-q";
    let quit = KeyBinding::new(quit_key, Quit, None);
    logged.push((quit_key, quit.action().name(), ""));
    bindings.push(quit);
    cx.bind_keys(bindings);
    for (keys, action, context) in logged {
        tracing::debug!(keys, action, context, "bound");
    }
}

/// Returns the node currently filling the body for extension controls.
pub fn zoomed(cx: &App) -> Option<String> {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.zoomed.as_ref().map(ToString::to_string))
}

/// Returns the node currently shown by a tabs container.
pub fn selected_tab(cx: &App, tabs: &str) -> Option<NodeId> {
    let node = NodeId::parse(tabs).ok()?;
    cx.try_global::<Registry>()?
        .tabs
        .iter()
        .filter_map(|tabs| tabs.upgrade())
        .find_map(|tabs| {
            let tabs = tabs.read(cx);
            (tabs.node == node).then(|| tabs.selected.clone()).flatten()
        })
}

#[derive(Clone, Copy)]
enum LayoutAction {
    ToggleZoom,
    Hide,
    SelectNextTab,
    SelectPreviousTab,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
}

impl LayoutAction {
    fn name(self) -> &'static str {
        match self {
            Self::ToggleZoom => "ToggleZoom",
            Self::Hide => "Hide",
            Self::SelectNextTab => "SelectNextTab",
            Self::SelectPreviousTab => "SelectPreviousTab",
            Self::FocusLeft => "FocusLeft",
            Self::FocusRight => "FocusRight",
            Self::FocusUp => "FocusUp",
            Self::FocusDown => "FocusDown",
        }
    }
}

fn layout_action(action: LayoutAction, node: Option<&NodeId>, window: &mut Window, cx: &mut App) {
    let _group = tracing::info_span!(
        "layout-action",
        action = action.name(),
        node = node.map_or("none", NodeId::as_str),
    )
    .entered();
    let Some(node) = node else {
        tracing::info!(reason = "no node has the focus", "refused");
        return;
    };
    let shell = Extension {
        name: "riso".into(),
    };
    let applied = match action {
        LayoutAction::ToggleZoom => {
            let previous = zoomed(cx);
            if previous.is_some() {
                shell.zoom_out(cx);
            } else {
                shell.set_zoomed(cx, node.as_str());
            }
            zoomed(cx) != previous
        }
        LayoutAction::Hide => {
            shell.set_hidden(cx, node.as_str(), true);
            if is_hidden(cx, node.as_str()) {
                if let Some(handle) = cx.global::<Registry>().root_focus_handle.clone() {
                    handle.focus(window, cx);
                }
                true
            } else {
                false
            }
        }
        LayoutAction::FocusLeft
        | LayoutAction::FocusRight
        | LayoutAction::FocusUp
        | LayoutAction::FocusDown => {
            focus_by_direction(action, node, window, cx);
            return;
        }
        LayoutAction::SelectNextTab | LayoutAction::SelectPreviousTab => select_adjacent_tab(
            &shell,
            cx,
            window,
            node,
            matches!(action, LayoutAction::SelectNextTab),
        ),
    };
    if applied {
        tracing::debug!("done");
    }
}

fn focus_by_direction(action: LayoutAction, node: &NodeId, window: &mut Window, cx: &mut App) {
    let registry = cx.global::<Registry>();
    let reason = match action {
        LayoutAction::FocusLeft => "no node on the left",
        LayoutAction::FocusRight => "no node on the right",
        LayoutAction::FocusUp => "no node above",
        LayoutAction::FocusDown => "no node below",
        _ => unreachable!(),
    };
    let Some(bounds) = registry.bounds.get(node) else {
        tracing::info!(reason = "no bounds for the focused node", "refused");
        return;
    };
    let center = bounds.center();
    let distance = gpui::px(4.);
    let mut point = match action {
        LayoutAction::FocusLeft => Point::new(bounds.left() - distance, center.y),
        LayoutAction::FocusRight => Point::new(bounds.right() + distance, center.y),
        LayoutAction::FocusUp => Point::new(center.x, bounds.top() - distance),
        LayoutAction::FocusDown => Point::new(center.x, bounds.bottom() + distance),
        _ => unreachable!(),
    };
    let mut shown = BTreeSet::new();
    find_in_drawn::<()>(registry, cx, |node| {
        shown.insert(node.clone());
        None
    });
    let window_bounds = Bounds::new(Point::default(), window.viewport_size());
    let target = loop {
        let layers = registry.tree.borrow();
        let tree = layers.current();
        let mut inside_shown = window_bounds.contains(&point);
        let target = find_in_drawn(registry, cx, |candidate| {
            let candidate_bounds = registry.bounds.get(candidate);
            inside_shown |= candidate_bounds.is_some_and(|bounds| bounds.contains(&point));
            let container = component_of(tree, candidate).is_some_and(|component| {
                matches!(
                    component.as_str(),
                    "riso.row"
                        | "riso.column"
                        | "riso.tabs"
                        | "riso.window"
                        | "riso.title_bar"
                        | "riso.status_bar"
                        | "riso.body"
                )
            });
            let has_shown_child = tree
                .slots(candidate)
                .any(|(_, children)| children.iter().any(|child| shown.contains(&child.id)));
            if container || has_shown_child {
                return None;
            }
            let bounds = registry.bounds.get(candidate);
            tracing::trace!(candidate = %candidate, bounds = ?bounds, "considered");
            bounds
                .is_some_and(|bounds| bounds.contains(&point))
                .then(|| candidate.clone())
        });
        if target.is_some() || !inside_shown {
            break target;
        }
        match action {
            LayoutAction::FocusLeft => point.x -= distance,
            LayoutAction::FocusRight => point.x += distance,
            LayoutAction::FocusUp => point.y -= distance,
            LayoutAction::FocusDown => point.y += distance,
            _ => unreachable!(),
        }
    };
    if let Some(target) = target
        && let Some(handle) = registry.focus_handles.get(&target).cloned()
    {
        handle.focus(window, cx);
        tracing::debug!(target = %target, "done");
        return;
    }
    tracing::info!(reason, bounds = ?bounds, point = ?point, "refused");
}

fn select_adjacent_tab(
    shell: &Extension,
    cx: &mut App,
    window: &mut Window,
    node: &NodeId,
    next: bool,
) -> bool {
    let tabs = {
        let registry = cx.global::<Registry>();
        let layers = registry.tree.borrow();
        let tree = layers.current();
        let mut current = node.clone();
        loop {
            if component_of(tree, &current)
                .is_some_and(|component| component.as_str() == "riso.tabs")
            {
                break registry
                    .tabs
                    .iter()
                    .filter_map(|tabs| tabs.upgrade())
                    .find(|tabs| tabs.read(cx).node == current);
            }
            let Some((parent, _, _, _)) = current_location(tree, &current) else {
                break None;
            };
            current = parent;
        }
    };
    let Some(tabs) = tabs else {
        tracing::info!(reason = "no tabs container holds it", "refused");
        return false;
    };
    let selected = tabs.update(cx, |tabs, cx| {
        tabs.reconcile_selection();
        let nodes = tabs.slots.nodes("children");
        if nodes.is_empty() {
            tracing::info!(reason = "no tabs", "refused");
            return None;
        }
        if nodes.len() == 1 {
            tracing::info!(reason = "one tab", "refused");
            return None;
        }
        let index = nodes
            .iter()
            .position(|node| Some(node) == tabs.selected.as_ref())?;
        let adjacent = if next {
            (index + 1) % nodes.len()
        } else {
            (index + nodes.len() - 1) % nodes.len()
        };
        let zoom_inside = {
            let registry = cx.global::<Registry>();
            let layers = registry.tree.borrow();
            let tree = layers.current();
            let mut current = registry.zoomed.clone();
            let mut inside = false;
            while let Some(node) = current {
                let Some((parent, _, _, _)) = current_location(tree, &node) else {
                    break;
                };
                if parent == tabs.node {
                    inside = true;
                    break;
                }
                current = Some(parent);
            }
            inside
        };
        if zoom_inside {
            shell.zoom_out(cx);
        }
        tabs.select(nodes[adjacent].clone(), cx);
        Some(tabs.node.clone())
    });
    if let Some(node) = selected {
        if let Some(handle) = cx
            .default_global::<Registry>()
            .focus_handles
            .get(&node)
            .cloned()
        {
            handle.focus(window, cx);
        }
        true
    } else {
        false
    }
}

/// Returns the innermost shown node that contains the keyboard focus.
pub fn focused_node(window: &Window, cx: &App) -> Option<NodeId> {
    window.focused(cx)?;
    let registry = cx.try_global::<Registry>()?;
    find_in_drawn(registry, cx, |node| {
        (!is_frame_node(node)
            && registry
                .focus_handles
                .get(node)
                .is_some_and(|handle| handle.contains_focused(window, cx)))
        .then(|| node.clone())
    })
}

/// Returns the last drawn bounds of a shown node.
pub fn node_bounds(cx: &App, node: &str) -> Option<Bounds<Pixels>> {
    let target = NodeId::parse(node).ok()?;
    let registry = cx.try_global::<Registry>()?;
    find_in_drawn(registry, cx, |current| {
        (current == &target)
            .then(|| registry.bounds.get(current).copied())
            .flatten()
    })
}

/// Returns a shown node's focus handle.
pub fn focus_handle(cx: &App, node: &str) -> Option<FocusHandle> {
    let target = NodeId::parse(node).ok()?;
    if is_frame_node(&target) {
        return None;
    }
    let registry = cx.try_global::<Registry>()?;
    find_in_drawn(registry, cx, |current| {
        (current == &target)
            .then(|| registry.focus_handles.get(current).cloned())
            .flatten()
    })
}

fn find_in_drawn<T>(
    registry: &Registry,
    cx: &App,
    mut visit: impl FnMut(&NodeId) -> Option<T>,
) -> Option<T> {
    let tabs_selected: BTreeMap<NodeId, Option<NodeId>> = registry
        .tabs
        .iter()
        .filter_map(|weak| weak.upgrade())
        .map(|tabs| {
            let tabs = tabs.read(cx);
            (tabs.node.clone(), tabs.selected.clone())
        })
        .collect();
    let layers = registry.tree.borrow();
    let tree = layers.current();
    let views = registry.views.borrow();
    let drawn_root = tree.root().map(|root| &root.id)?;
    let zoomed = registry
        .zoomed
        .as_ref()
        .filter(|node| views.contains_key(*node));
    fn walk<T>(
        tree: &Tree,
        views: &ViewMap,
        tabs_selected: &BTreeMap<NodeId, Option<NodeId>>,
        zoomed: Option<&NodeId>,
        current: &NodeId,
        visit: &mut impl FnMut(&NodeId) -> Option<T>,
    ) -> Option<T> {
        if tree.is_hidden(current) || !views.contains_key(current) {
            return None;
        }
        let from_child = if current.as_str() == "body"
            && let Some(zoomed_node) = zoomed
        {
            walk(tree, views, tabs_selected, None, zoomed_node, visit)
        } else if let Some(selected) = tabs_selected.get(current) {
            selected
                .as_ref()
                .and_then(|child| walk(tree, views, tabs_selected, zoomed, child, visit))
        } else {
            tree.slots(current).find_map(|(_, children)| {
                children
                    .iter()
                    .find_map(|child| walk(tree, views, tabs_selected, zoomed, &child.id, visit))
            })
        };
        from_child.or_else(|| visit(current))
    }
    walk(tree, &views, &tabs_selected, zoomed, drawn_root, &mut visit)
}

fn save(cx: &App) {
    let Some((path, len, encoded)) = cx.try_global::<Registry>().and_then(|registry| {
        if !registry.default_fixed {
            return None;
        }
        let path = registry.layout_file.clone()?;
        let layers = registry.tree.borrow();
        let ops = layers.user_layer();
        Some((path, ops.len(), encode(ops)))
    }) else {
        return;
    };
    let _group = tracing::info_span!("layout-save", operations = len).entered();
    if write_atomic(&path, encoded.as_bytes()).is_ok() {
        tracing::debug!("saved");
    }
}

fn is_shown(tree: &Tree, node: &NodeId) -> bool {
    fn find(tree: &Tree, current: &NodeId, target: &NodeId) -> bool {
        !tree.is_hidden(current)
            && (current == target
                || tree.slots(current).any(|(_, children)| {
                    children.iter().any(|child| find(tree, &child.id, target))
                }))
    }
    tree.root().is_some_and(|root| find(tree, &root.id, node))
}

fn node_of<'a>(tree: &'a Tree, id: &NodeId) -> Option<&'a Node> {
    fn find<'a>(tree: &'a Tree, node: &'a Node, id: &NodeId) -> Option<&'a Node> {
        if &node.id == id {
            return Some(node);
        }
        tree.slots(&node.id)
            .flat_map(|(_, children)| children)
            .find_map(|child| find(tree, child, id))
    }
    find(tree, tree.root()?, id)
}

fn component_of<'a>(tree: &'a Tree, id: &NodeId) -> Option<&'a ComponentId> {
    node_of(tree, id).map(|node| &node.component)
}

fn collect_nodes(tree: &Tree, node: &Node, out: &mut Vec<Node>) {
    out.push(node.clone());
    for (_, children) in tree.slots(&node.id) {
        for child in children {
            collect_nodes(tree, child, out);
        }
    }
}

fn encode_props(props: impl serde::Serialize) -> Result<Props, String> {
    match serde_json::to_value(props) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => Err("props must encode as a JSON object".into()),
        Err(error) => Err(format!("props cannot be encoded as JSON: {error}")),
    }
}

fn read_props<P>(node: &NodeId, component: &ComponentId, props: &Props) -> P
where
    P: serde::Serialize + serde::de::DeserializeOwned + Default,
{
    let _group = tracing::info_span!(
        "props-read",
        node = node.as_str(),
        component = component.as_str(),
    )
    .entered();
    let default = P::default();
    let default_map = match serde_json::to_value(&default) {
        Ok(serde_json::Value::Object(map)) => map,
        Ok(_) => {
            log::error!("default props of component {component} did not encode as a JSON object");
            return default;
        }
        Err(error) => {
            log::error!("default props of component {component} cannot be encoded: {error}");
            return default;
        }
    };
    let mut merged = default_map.clone();
    let mut accepted = Vec::new();
    for (key, value) in props {
        if !default_map.contains_key(key) {
            log::warn!("node {node} has an unknown prop {key}");
            continue;
        }
        let mut candidate = default_map.clone();
        candidate.insert(key.clone(), value.clone());
        match serde_json::from_value::<P>(serde_json::Value::Object(candidate)) {
            Ok(_) => {
                merged.insert(key.clone(), value.clone());
                accepted.push(key.as_str());
            }
            Err(error) => {
                log::warn!(
                    "prop {key} of node {node} cannot be read: {error}; the default is used"
                );
            }
        }
    }
    let keys = accepted.join(",");
    tracing::debug!(keys, "props read");
    serde_json::from_value::<P>(serde_json::Value::Object(merged)).unwrap_or(default)
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
    let target_node = node_id.clone();
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
                let dropped = {
                    let registry = cx.default_global::<Registry>();
                    registry.focus_handles.remove(&id);
                    registry.bounds.remove(&id);
                    registry.tellers.remove(&id);
                    registry.views.borrow_mut().remove(&id)
                };
                if dropped.is_some() {
                    drop(dropped);
                    tracing::debug!(node = %id, "view dropped");
                }
            }
        }
        Applied::PropsSet => {
            let tell = {
                let registry = cx.default_global::<Registry>();
                let teller = registry.tellers.get(&target_node).cloned();
                let props = teller.as_ref().and_then(|_| {
                    let layers = registry.tree.borrow();
                    node_of(layers.current(), &target_node).map(|node| node.props.clone())
                });
                teller.zip(props)
            };
            if let Some((teller, props)) = tell {
                teller(&props, cx);
            }
        }
        Applied::Moved | Applied::SetHidden(_) => {}
    }
    {
        let registry = cx.default_global::<Registry>();
        if registry.zoomed.as_ref().is_some_and(|node| {
            let layers = registry.tree.borrow();
            let tree = layers.current();
            !is_shown(tree, node) || !is_in_body(tree, node)
        }) {
            let node = registry.zoomed.take().unwrap();
            tracing::debug!(node = %node, "zoom cleared");
        }
    }
    reconcile_containers(cx);
    cx.refresh_windows();
    save(cx);
}

fn slot_count_rule_error(
    registry: &Registry,
    parent: &NodeId,
    slot: &SlotName,
    moving: Option<&NodeId>,
) -> Option<String> {
    let layers = registry.tree.borrow();
    let tree = layers.current();
    let component = component_of(tree, parent)?;
    if !registry
        .slots_for_one
        .contains(&(component.clone(), slot.clone()))
    {
        return None;
    }
    let held = tree
        .slots(parent)
        .find(|(name, _)| *name == slot)?
        .1
        .first()?;
    if moving == Some(&held.id) {
        return None;
    }
    Some(format!(
        "{parent}.{slot} holds one node, and already holds {}",
        held.id
    ))
}

fn check_shell_rules(extension: &str, cx: &mut App, operation: &Operation) -> bool {
    let registry = cx.default_global::<Registry>();
    match operation {
        Operation::Add {
            node, parent, slot, ..
        } => {
            if let Some(reason) = slot_count_rule_error(registry, parent, slot, None) {
                tracing::info!("{reason}");
                return false;
            }

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
            if is_frame_component(&node.component) || is_frame_slot(parent, slot) {
                tracing::info!(
                    node = %node.id,
                    component = %node.component,
                    parent = %parent,
                    slot = %slot,
                    "the window's frame belongs to the shell",
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
            let layers = registry.tree.borrow();
            let tree = layers.current();
            if let Some(parent_component) = component_of(tree, parent)
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
            node,
            parent,
            slot,
            at,
        } => {
            if let Some(reason) = slot_count_rule_error(registry, parent, slot, Some(node)) {
                tracing::info!("{reason}");
                return false;
            }
            if is_frame_node(node) || is_frame_slot(parent, slot) {
                tracing::info!(
                    node = %node,
                    parent = %parent,
                    slot = %slot,
                    index = %index_field(at),
                    "the window's frame belongs to the shell",
                );
                return false;
            }
            if registry.extension_root.as_ref() == Some(node) {
                tracing::info!(
                    node = %node,
                    parent = %parent,
                    slot = %slot,
                    index = %index_field(at),
                    "the root cannot be moved",
                );
                return false;
            }
            let layers = registry.tree.borrow();
            let tree = layers.current();
            if let Some(parent_component) = component_of(tree, parent)
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
        Operation::Remove { node } => {
            if is_frame_node(node) {
                tracing::info!(node = %node, "the window's frame belongs to the shell");
                return false;
            }
            if registry.extension_root.as_ref() == Some(node) {
                tracing::info!(node = %node, "the root cannot be removed");
                return false;
            }
            if let Some(reason) = required_rule_error(registry, operation) {
                tracing::info!("{reason}");
                return false;
            }
            true
        }
        Operation::SetHidden { node, .. } => {
            if is_frame_node(node) {
                tracing::info!(node = %node, "the window's frame belongs to the shell");
                return false;
            }
            if registry.extension_root.as_ref() == Some(node) {
                tracing::info!(node = %node, "the root cannot be hidden or shown");
                return false;
            }
            if let Some(reason) = required_rule_error(registry, operation) {
                tracing::info!("{reason}");
                return false;
            }
            true
        }
        Operation::SetProps { node, .. } => {
            if is_frame_node(node) {
                tracing::info!(node = %node, "the window's frame belongs to the shell");
                return false;
            }
            true
        }
    }
}

fn shell_rule_error(extension: &str, cx: &mut App, operation: &Operation) -> Result<(), String> {
    let registry = cx.default_global::<Registry>();
    match operation {
        Operation::Add {
            node, parent, slot, ..
        } => {
            if let Some(reason) = slot_count_rule_error(registry, parent, slot, None) {
                return Err(reason);
            }

            let namespace = node.component.as_str().split('.').next();
            if namespace != Some(extension) && namespace != Some("riso") {
                return Err("component is not in the extension's namespace".into());
            }
            if is_frame_component(&node.component) || is_frame_slot(parent, slot) {
                return Err("the window's frame belongs to the shell".into());
            }
            if !registry.components.contains_key(&node.component) {
                return Err("component is not registered".into());
            }
            let layers = registry.tree.borrow();
            let tree = layers.current();
            if let Some(parent_component) = component_of(tree, parent)
                && !registry
                    .components
                    .get(parent_component)
                    .is_some_and(|component| component.slots.contains(slot))
            {
                return Err("parent component does not declare slot".into());
            }
            Ok(())
        }
        Operation::Move {
            node, parent, slot, ..
        } => {
            if let Some(reason) = slot_count_rule_error(registry, parent, slot, Some(node)) {
                return Err(reason);
            }
            if is_frame_node(node) || is_frame_slot(parent, slot) {
                return Err("the window's frame belongs to the shell".into());
            }
            if registry.extension_root.as_ref() == Some(node) {
                return Err("the root cannot be removed, moved, hidden, or shown".into());
            }
            let layers = registry.tree.borrow();
            let tree = layers.current();
            if let Some(parent_component) = component_of(tree, parent)
                && !registry
                    .components
                    .get(parent_component)
                    .is_some_and(|component| component.slots.contains(slot))
            {
                return Err("parent component does not declare slot".into());
            }
            Ok(())
        }
        Operation::Remove { node } | Operation::SetHidden { node, .. } => {
            if is_frame_node(node) {
                return Err("the window's frame belongs to the shell".into());
            }
            if registry.extension_root.as_ref() == Some(node) {
                return Err("the root cannot be removed, moved, hidden, or shown".into());
            }
            required_rule_error(registry, operation).map_or(Ok(()), Err)
        }
        Operation::SetProps { node, .. } => {
            if is_frame_node(node) {
                return Err("the window's frame belongs to the shell".into());
            }
            Ok(())
        }
    }
}

/// Why a `hide` or `remove` would take a node of a required component off
/// screen, if it would. The default layout is not checked.
fn required_rule_error(registry: &Registry, operation: &Operation) -> Option<String> {
    let (node, verb) = match operation {
        Operation::Remove { node } => (node, "removed"),
        Operation::SetHidden { node, hidden: true } => (node, "hidden"),
        _ => return None,
    };
    if !registry.default_fixed || registry.required.is_empty() {
        return None;
    }
    fn first_required<'a>(registry: &Registry, tree: &'a Tree, node: &'a Node) -> Option<&'a Node> {
        if registry.required.contains(&node.component) {
            return Some(node);
        }
        tree.slots(&node.id)
            .flat_map(|(_, children)| children)
            .find_map(|child| first_required(registry, tree, child))
    }
    let layers = registry.tree.borrow();
    let tree = layers.current();
    let required = first_required(registry, tree, node_of(tree, node)?)?;
    Some(if &required.id == node {
        format!("{node} cannot be {verb}: it is required")
    } else {
        format!(
            "{node} cannot be {verb}: it holds the required node {}",
            required.id
        )
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
        self.add_internal(window, cx, node, component, parent, slot, Ok(Props::new()));
    }

    /// Adds a new node with props at the end of a parent's declared slot and creates its view.
    #[allow(clippy::too_many_arguments)]
    pub fn add_with_props(
        &self,
        window: &mut Window,
        cx: &mut App,
        node: &str,
        component: &str,
        parent: &str,
        slot: &str,
        props: impl serde::Serialize,
    ) {
        self.add_internal(
            window,
            cx,
            node,
            component,
            parent,
            slot,
            encode_props(props),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn add_internal(
        &self,
        window: &mut Window,
        cx: &mut App,
        node: &str,
        component: &str,
        parent: &str,
        slot: &str,
        props: Result<Props, String>,
    ) {
        run_op(
            &self.name,
            Some(window),
            cx,
            "add",
            node,
            |extension, id| {
                let props = props?;
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
                        props,
                    },
                    parent,
                    slot,
                    at: Position::Last,
                })
            },
        );
    }

    /// Sets or clears keys on any node's props and tells its view.
    pub fn set_props(&self, cx: &mut App, node: &str, props: impl serde::Serialize) {
        let props = encode_props(props);
        run_op(&self.name, None, cx, "props", node, |_, node| {
            Ok(Operation::SetProps {
                node,
                props: props?,
            })
        });
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
                at: Position::Index(index),
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

    /// Zooms any shown non-root node without changing the layout tree.
    pub fn set_zoomed(&self, cx: &mut App, node: &str) {
        let _group = tracing::info_span!("zoom", node, extension = self.name.as_str()).entered();
        let id = match NodeId::parse(node) {
            Ok(id) => id,
            Err(error) => {
                log::error!("invalid node ID {node:?}: {error}");
                return;
            }
        };
        let registry = cx.default_global::<Registry>();
        {
            let layers = registry.tree.borrow();
            let tree = layers.current();
            if component_of(tree, &id).is_none() {
                tracing::info!("the node is not in the tree");
                return;
            }
            if is_frame_node(&id) {
                tracing::info!("the window's frame belongs to the shell");
                return;
            }
            if !is_in_body(tree, &id) {
                tracing::info!("only a node in the body can be zoomed");
                return;
            }
            if registry.extension_root.as_ref() == Some(&id) {
                tracing::info!("the root already fills the body");
                return;
            }
            if !is_shown(tree, &id) {
                tracing::info!("the node is hidden or under a hidden node");
                return;
            }
        }
        if registry.views_created && !registry.views.borrow().contains_key(&id) {
            tracing::info!("the node has no view");
            return;
        }
        if let Some(previous) = registry.zoomed.replace(id.clone())
            && previous != id
        {
            tracing::debug!(previous = %previous, "replaced");
        }
        tracing::debug!(node, "zoomed");
        cx.refresh_windows();
    }

    /// Restores the tree with its existing views, shares, and tab selection.
    pub fn zoom_out(&self, cx: &mut App) {
        let _group = tracing::info_span!("zoom-out", extension = self.name.as_str()).entered();
        let Some(node) = cx.default_global::<Registry>().zoomed.take() else {
            tracing::debug!("nothing zoomed");
            return;
        };
        tracing::debug!(node = %node, "zoomed out");
        cx.refresh_windows();
    }

    /// Drops runtime operations and reconciles the default tree and view state.
    pub fn reset_layout(&self, window: &mut Window, cx: &mut App) {
        let _group = tracing::info_span!("layout-reset", extension = self.name.as_str()).entered();
        let (added, before_props) = {
            let registry = cx.default_global::<Registry>();
            let layers = registry.tree.borrow();
            let added: BTreeSet<_> = layers
                .user_layer()
                .iter()
                .filter_map(|op| match op {
                    Operation::Add { node, .. } => Some(node.id.clone()),
                    _ => None,
                })
                .collect();
            let mut before = Vec::new();
            if let Some(root) = layers.current().root() {
                collect_nodes(layers.current(), root, &mut before);
            }
            let before_props: BTreeMap<NodeId, Props> = before
                .into_iter()
                .map(|node| (node.id, node.props))
                .collect();
            (added, before_props)
        };
        if cx.default_global::<Registry>().tree.borrow_mut().reset() == 0 {
            return;
        }
        let (root, kept) = {
            let registry = cx.default_global::<Registry>();
            let layers = registry.tree.borrow();
            let tree = layers.current();
            let removed: Vec<_> = registry
                .views
                .borrow()
                .keys()
                .filter(|id| added.contains(*id) || component_of(tree, id).is_none())
                .cloned()
                .collect();
            for id in removed {
                registry.focus_handles.remove(&id);
                registry.bounds.remove(&id);
                registry.tellers.remove(&id);
                let dropped = registry.views.borrow_mut().remove(&id);
                drop(dropped);
                tracing::debug!(node = %id, "view dropped");
            }
            let kept: BTreeSet<_> = registry.views.borrow().keys().cloned().collect();
            (tree.root().cloned(), kept)
        };
        if let Some(root) = root {
            let _ = create_view(&root, None, window, cx);
        }
        let to_tell: Vec<(Teller, Props)> = {
            let registry = cx.default_global::<Registry>();
            let layers = registry.tree.borrow();
            let tree = layers.current();
            let mut after = Vec::new();
            if let Some(root) = tree.root() {
                collect_nodes(tree, root, &mut after);
            }
            after
                .into_iter()
                .filter(|node| {
                    kept.contains(&node.id) && before_props.get(&node.id) != Some(&node.props)
                })
                .filter_map(|node| {
                    registry
                        .tellers
                        .get(&node.id)
                        .cloned()
                        .map(|teller| (teller, node.props.clone()))
                })
                .collect()
        };
        for (teller, props) in to_tell {
            teller(&props, cx);
        }
        let (splits, tabs) = {
            let registry = cx.default_global::<Registry>();
            registry.splits.retain(|split| split.upgrade().is_some());
            registry.tabs.retain(|tabs| tabs.upgrade().is_some());
            registry.zoomed = None;
            (registry.splits.clone(), registry.tabs.clone())
        };
        for split in splits {
            let _ = split.update(cx, |split, _| {
                split.shares.clear();
                split.shown_nodes.clear();
                split.shown_shares = Rc::new(RefCell::new(Vec::new()));
                split.edges.clear();
                split.child_bounds.clear();
                split.reconcile();
            });
        }
        for tabs in tabs {
            let _ = tabs.update(cx, |tabs, _| {
                tabs.selected = tabs.slots.nodes("children").first().cloned();
            });
        }
        cx.refresh_windows();
        save(cx);
    }

    /// Registers the first factory for a valid component ID.
    pub fn register(
        &self,
        cx: &mut App,
        component: &str,
        slots: &[&str],
        create: impl Fn(&NodeId, Slots, &mut Window, &mut App) -> AnyView + 'static,
    ) {
        self.register_factory(
            cx,
            component,
            slots,
            Rc::new(move |node, slots, window, cx| create(&node.id, slots, window, cx)),
        );
    }

    /// Declares a slot of this extension's component a slot for one node.
    pub fn slot_for_one(&self, cx: &mut App, component: &str, slot: &str) {
        let _group = tracing::info_span!("slot-declare", component, slot).entered();
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
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
        if component.as_str().split('.').next() != Some(self.name.as_str()) {
            log::error!(
                "extension {} cannot declare slot {component}.{slot} for one",
                self.name
            );
            return;
        }
        let registry = cx.default_global::<Registry>();
        if let Some(registered) = registry.components.get(&component)
            && !registered.slots.contains(&slot)
        {
            log::error!("component {component} does not declare slot {slot}");
            return;
        }
        if registry
            .slots_for_one
            .contains(&(component.clone(), slot.clone()))
        {
            log::error!("slot {component}.{slot} is already declared for one");
            return;
        }
        registry.slots_for_one.push((component, slot));
        tracing::debug!("slot for one");
    }

    /// Declares a component of this extension required: its nodes, and the
    /// nodes above them, cannot be hidden or removed.
    pub fn require(&self, cx: &mut App, component: &str) {
        let _group = tracing::info_span!("component-require", component).entered();
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
                return;
            }
        };
        if component.as_str().split('.').next() != Some(self.name.as_str()) {
            log::error!(
                "extension {} cannot require component {component}",
                self.name
            );
            return;
        }
        if cx
            .default_global::<Registry>()
            .required
            .insert(component.clone())
        {
            tracing::debug!("required");
        } else {
            log::error!("component {component} is already required");
        }
    }

    /// Registers the view type that supplies a component's titles.
    pub fn register_title<V: Titled + 'static>(&self, cx: &mut App, component: &str) {
        let _group = tracing::info_span!("title-register", component).entered();
        let component = match ComponentId::parse(component) {
            Ok(component) => component,
            Err(error) => {
                log::error!("invalid component ID {component:?}: {error}");
                return;
            }
        };
        if component.as_str().split('.').next() != Some(self.name.as_str()) {
            log::error!(
                "extension {} cannot register titles for component {component}",
                self.name
            );
            return;
        }
        match cx.default_global::<Registry>().titles.entry(component) {
            Entry::Vacant(entry) => {
                entry.insert(Rc::new(|view, cx| {
                    let entity = view.clone().downcast::<V>().ok()?;
                    let view = entity.read(cx);
                    Some((view.title(cx), view.icon(cx)))
                }));
                tracing::debug!("gives titles");
            }
            Entry::Occupied(entry) => log::error!("component {} already gives titles", entry.key()),
        }
    }

    /// Registers a component whose view is created from and told typed props.
    pub fn register_with_props<P, V>(
        &self,
        cx: &mut App,
        component: &str,
        slots: &[&str],
        create: impl Fn(P, &NodeId, Slots, &mut Window, &mut Context<V>) -> V + 'static,
    ) where
        P: serde::Serialize + serde::de::DeserializeOwned + Default + 'static,
        V: PropsView<P> + 'static,
    {
        self.register_factory(
            cx,
            component,
            slots,
            Rc::new(move |node, slots, window, cx| {
                let parsed = read_props::<P>(&node.id, &node.component, &node.props);
                let node_id = node.id.clone();
                let entity = cx.new(|cx| create(parsed, &node_id, slots, window, cx));
                let weak = entity.downgrade();
                let tell_node = node.id.clone();
                let tell_component = node.component.clone();
                let teller: Teller = Rc::new(move |props, cx| {
                    let Some(entity) = weak.upgrade() else {
                        return;
                    };
                    let parsed = read_props::<P>(&tell_node, &tell_component, props);
                    entity.update(cx, |view, cx| {
                        view.set_props(parsed, cx);
                        cx.notify();
                    });
                    tracing::debug!(node = %tell_node, "view told");
                });
                cx.default_global::<Registry>()
                    .tellers
                    .insert(node.id.clone(), teller);
                entity.into()
            }),
        );
    }

    fn register_factory(&self, cx: &mut App, component: &str, slots: &[&str], create: Factory) {
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
        let registry = cx.default_global::<Registry>();
        if !registry.components.contains_key(&component) {
            registry.slots_for_one.retain(|(declared_component, slot)| {
                if declared_component == &component && !names.contains(slot) {
                    log::error!("component {component} does not declare slot {slot}");
                    false
                } else {
                    true
                }
            });
        }
        match registry.components.entry(component) {
            Entry::Vacant(entry) => {
                entry.insert(Component {
                    slots: names,
                    create,
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
        self.place_root_internal(cx, node, component, Ok(Props::new()));
    }

    /// Places a valid root with props, whether or not its component is registered yet.
    pub fn place_root_with_props(
        &self,
        cx: &mut App,
        node: &str,
        component: &str,
        props: impl serde::Serialize,
    ) {
        self.place_root_internal(cx, node, component, encode_props(props));
    }

    fn place_root_internal(
        &self,
        cx: &mut App,
        node: &str,
        component: &str,
        props: Result<Props, String>,
    ) {
        let _group = tracing::info_span!("root-place", node, component).entered();
        if cx.default_global::<Registry>().default_fixed {
            tracing::info!("the window is open; add the node with add");
            return;
        }
        let props = match props {
            Ok(props) => props,
            Err(message) => {
                log::error!("{message}");
                return;
            }
        };
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
        if is_frame_component(&component) {
            tracing::info!(
                node = %id,
                component = %component,
                extension = %self.name,
                "the window's frame belongs to the shell",
            );
            return;
        }
        let registry = cx.default_global::<Registry>();
        if let Some(root) = &registry.extension_root {
            tracing::info!(
                node = %id,
                component = %component,
                extension = %self.name,
                root = %root,
                "a root is already placed",
            );
            return;
        }
        let body = NodeId::parse("body").unwrap();
        let children = SlotName::parse("children").unwrap();
        let placed = registry
            .tree
            .borrow_mut()
            .current_mut()
            .expect("startup placement is open")
            .place(
                Node {
                    id: id.clone(),
                    component,
                    extension: self.name.clone(),
                    props,
                },
                &body,
                children,
            );
        if placed.is_ok() {
            registry.extension_root = Some(id);
        }
    }

    /// Places a node under an already placed parent.
    pub fn place(&self, cx: &mut App, node: &str, component: &str, parent: &str, slot: &str) {
        self.place_internal(cx, node, component, parent, slot, Ok(Props::new()));
    }

    /// Places a node with props under an already placed parent.
    pub fn place_with_props(
        &self,
        cx: &mut App,
        node: &str,
        component: &str,
        parent: &str,
        slot: &str,
        props: impl serde::Serialize,
    ) {
        self.place_internal(cx, node, component, parent, slot, encode_props(props));
    }

    fn place_internal(
        &self,
        cx: &mut App,
        node: &str,
        component: &str,
        parent: &str,
        slot: &str,
        props: Result<Props, String>,
    ) {
        let _group = tracing::info_span!("node-place", node, component, parent, slot).entered();
        if cx.default_global::<Registry>().default_fixed {
            tracing::info!("the window is open; add the node with add");
            return;
        }
        let props = match props {
            Ok(props) => props,
            Err(message) => {
                log::error!("{message}");
                return;
            }
        };
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
        if is_frame_component(&component) || is_frame_slot(&parent, &slot) {
            tracing::info!(
                node = %id,
                component = %component,
                extension = %self.name,
                parent = %parent,
                slot = %slot,
                "the window's frame belongs to the shell",
            );
            return;
        }
        if let Some(reason) =
            slot_count_rule_error(cx.default_global::<Registry>(), &parent, &slot, None)
        {
            tracing::info!("{reason}");
            return;
        }
        let _ = cx
            .default_global::<Registry>()
            .tree
            .borrow_mut()
            .current_mut()
            .expect("startup placement is open")
            .place(
                Node {
                    id,
                    component,
                    extension: self.name.clone(),
                    props,
                },
                &parent,
                slot,
            );
    }

    /// Binds default keys for actions in the given key contexts.
    pub fn bind_keys(&self, cx: &mut App, bindings: &[(&str, Box<dyn Action>, &str)]) {
        let _group = tracing::info_span!(
            "keys-bind",
            extension = self.name.as_str(),
            count = bindings.len(),
        )
        .entered();
        let mut valid = Vec::with_capacity(bindings.len());
        let mut logged = Vec::with_capacity(bindings.len());
        for (keys, action, context) in bindings {
            let predicate = match KeyBindingContextPredicate::parse(context) {
                Ok(predicate) => Rc::new(predicate),
                Err(error) => {
                    log::error!("invalid key context {context:?} for keys {keys:?}: {error}");
                    continue;
                }
            };
            if keys.split_whitespace().next().is_none() {
                log::error!("invalid key binding {keys:?}: empty key sequence");
                continue;
            }
            let action_name = action.name();
            let binding = match KeyBinding::load(
                keys,
                action.boxed_clone(),
                Some(predicate),
                false,
                None,
                cx,
            ) {
                Ok(binding) => binding,
                Err(error) => {
                    log::error!("invalid key binding {keys:?}: {error}");
                    continue;
                }
            };
            if binding
                .keystrokes()
                .iter()
                .any(|keystroke| keystroke.key().is_empty())
            {
                log::error!("invalid key binding {keys:?}: missing key after `-`");
                continue;
            }
            logged.push((*keys, action_name, *context));
            valid.push(binding);
        }
        if !valid.is_empty() {
            cx.bind_keys(valid);
        }
        for (keys, action, context) in logged {
            tracing::debug!(keys, action, context, "bound");
        }
    }

    /// Registers a window-wide handler for an action in the extension's namespace.
    pub fn on_action<A: Action>(
        &self,
        cx: &mut App,
        handler: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) {
        let action = A::name_for_type();
        if !action
            .strip_prefix(self.name.as_str())
            .is_some_and(|rest| rest.starts_with("::"))
        {
            log::error!("extension {} cannot handle {action}", self.name);
            return;
        }
        let handler = Rc::new(handler);
        let extension = self.name.clone();
        cx.default_global::<Registry>()
            .action_handlers
            .push(Rc::new(move |div| {
                let handler = handler.clone();
                let extension = extension.clone();
                div.on_action::<A>(move |value, window, cx| {
                    let _group = tracing::info_span!(
                        "extension-action",
                        extension = extension.as_str(),
                        action,
                    )
                    .entered();
                    handler(value, window, cx);
                })
            }));
        tracing::debug!(extension = self.name.as_str(), action, "action handled");
    }

    /// Shows a toast in this window.
    pub fn show_toast(&self, toast: riso_ui::Toast, window: &mut Window, cx: &mut App) {
        if let Some(layer) = toast_layer(window, cx) {
            layer.update(cx, |layer, cx| layer.show_toast(toast, cx));
        }
    }

    /// Closes the toast in this window, if one is shown.
    pub fn hide_toast(&self, window: &mut Window, cx: &mut App) {
        if let Some(layer) = toast_layer(window, cx) {
            layer.update(cx, |layer, cx| {
                layer.hide_toast(riso_ui::ToastHideReason::HideToast, cx)
            });
        }
    }

    /// Toggles a modal of type `V` in the window's modal layer.
    pub fn toggle_modal<V: riso_ui::ModalView>(
        &self,
        window: &mut Window,
        cx: &mut App,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) {
        let Some(layer) = modal_layer(window, cx) else {
            return;
        };
        layer.update(cx, |layer, cx| {
            layer.toggle_modal(window, cx, build);
        });
    }

    /// Closes the active modal in the window's modal layer, if one is open.
    pub fn hide_modal(&self, window: &mut Window, cx: &mut App) {
        let Some(layer) = modal_layer(window, cx) else {
            return;
        };
        layer.update(cx, |layer, cx| {
            layer.hide_modal(riso_ui::ModalHideReason::HideModal, window, cx);
        });
    }

    /// Registers the section `T`, whose key must be this extension's name.
    pub fn register_settings<T: riso_settings::Settings>(&self, cx: &mut App) {
        if T::KEY != self.name {
            log::error!(
                "extension {} cannot declare the section {}",
                self.name,
                T::KEY
            );
            return;
        }
        riso_settings::register::<T>(cx);
    }
}

fn toast_layer(window: &Window, cx: &App) -> Option<Entity<riso_ui::ToastLayer>> {
    Some(
        window
            .root::<Shell>()
            .flatten()?
            .read(cx)
            .toast_layer
            .clone(),
    )
}

/// Returns whether this window shows a toast.
pub fn has_toast(window: &Window, cx: &App) -> bool {
    toast_layer(window, cx).is_some_and(|layer| layer.read(cx).has_active_toast())
}

fn modal_layer(window: &Window, cx: &App) -> Option<Entity<riso_ui::ModalLayer>> {
    let shell = window.root::<Shell>().flatten()?;
    Some(shell.read(cx).modal_layer.clone())
}

/// Returns whether the window's modal layer currently shows a modal.
pub fn has_modal(window: &Window, cx: &App) -> bool {
    modal_layer(window, cx).is_some_and(|layer| layer.read(cx).has_active_modal())
}

/// The window's root view. All views live in the shell's global map.
pub struct Shell {
    handle: FocusHandle,
    root: Option<AnyView>,
    modal_layer: Entity<riso_ui::ModalLayer>,
    toast_layer: Entity<riso_ui::ToastLayer>,
    last_focused: Option<NodeId>,
    _focus_lost: Subscription,
}

impl Shell {
    fn sync_focus(&mut self, window: &Window, cx: &App) {
        let current = focused_node(window, cx);
        if self.last_focused == current {
            return;
        }
        self.last_focused = current.clone();
        let node = current.as_ref().map_or("none", NodeId::as_str);
        let _group = tracing::info_span!("focus-change", node = %node).entered();
        tracing::debug!("focused");
    }
}

/// Creates the initial nodes once, parent before children, when the window opens.
pub fn new_shell(window: &mut Window, cx: &mut App) -> Entity<Shell> {
    let should_restore = {
        let registry = cx.default_global::<Registry>();
        if !registry.default_fixed {
            registry.tree.borrow_mut().set_default();
            registry.default_fixed = true;
            true
        } else {
            false
        }
    };
    if should_restore {
        restore(cx);
    }
    let (frame_root, ext_root) = {
        let registry = cx.default_global::<Registry>();
        let layers = registry.tree.borrow();
        let tree = layers.current();
        (tree.root().cloned(), registry.extension_root.clone())
    };
    let _group = tracing::info_span!(
        "shell-create",
        root = %ext_root.as_ref().map_or("none", NodeId::as_str)
    )
    .entered();
    if ext_root.is_none() {
        tracing::info!("no root placed; the window shows the background");
    }
    let root = frame_root.and_then(|node| create_view(&node, None, window, cx));
    let registry = cx.default_global::<Registry>();
    registry.views_created = true;
    if registry.zoomed.as_ref().is_some_and(|node| {
        let layers = registry.tree.borrow();
        let tree = layers.current();
        !registry.views.borrow().contains_key(node)
            || !is_shown(tree, node)
            || !is_in_body(tree, node)
    }) {
        let node = registry.zoomed.take().unwrap();
        tracing::debug!(node = %node, "zoom cleared");
    }
    // Initialize split children and tab selections early so visibility checks work before the first draw.
    reconcile_containers(cx);
    let toast_layer = cx.new(|_| riso_ui::ToastLayer::new());
    let modal_layer = cx.new(|_| riso_ui::ModalLayer::new());
    cx.new(|cx| {
        let handle = cx.focus_handle();
        cx.default_global::<Registry>().root_focus_handle = Some(handle.clone());
        if window.focused(cx).is_none() {
            handle.focus(window, cx);
        }
        Shell {
            handle,
            root,
            modal_layer,
            toast_layer,
            last_focused: None,
            _focus_lost: cx.on_focus_lost(window, |shell: &mut Shell, window, cx| {
                shell.handle.focus(window, cx);
                shell.sync_focus(window, cx);
                window.on_next_frame(|window, _| window.refresh());
            }),
        }
    })
}

fn restore(cx: &mut App) {
    let Some(path) = cx.default_global::<Registry>().layout_file.clone() else {
        return;
    };
    let _group = tracing::info_span!("layout-restore", file = %path.display()).entered();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!("no saved layout");
            return;
        }
        Err(error) => {
            log::warn!("the saved layout cannot be read: {error}");
            return;
        }
    };
    let operations = match decode(&text) {
        Ok(operations) => operations,
        Err(error) => {
            log::warn!("the saved layout cannot be read: {error}");
            return;
        }
    };
    let mut applied = 0usize;
    let mut skipped = 0usize;
    for operation in operations {
        let (op, node) = match &operation {
            Operation::Add { node, .. } => ("add", node.id.clone()),
            Operation::Remove { node } => ("remove", node.clone()),
            Operation::Move { node, .. } => ("move", node.clone()),
            Operation::SetHidden { node, hidden: true } => ("hide", node.clone()),
            Operation::SetHidden {
                node,
                hidden: false,
            } => ("show", node.clone()),
            Operation::SetProps { node, .. } => ("props", node.clone()),
        };
        let extension = match &operation {
            Operation::Add { node, .. } => node.extension.as_str(),
            _ => "riso",
        };
        if let Err(reason) = shell_rule_error(extension, cx, &operation) {
            tracing::info!(
                op,
                node = node.as_str(),
                reason = reason.as_str(),
                "skipped"
            );
            skipped += 1;
            continue;
        }
        let result = {
            let tree = cx.default_global::<Registry>().tree.clone();
            let mut tree = tree.borrow_mut();
            tree.apply(operation)
        };
        match result {
            Ok(_) => applied += 1,
            Err(error) => {
                let reason = error.to_string();
                tracing::info!(
                    op,
                    node = node.as_str(),
                    reason = reason.as_str(),
                    "skipped"
                );
                skipped += 1;
            }
        }
    }
    tracing::debug!(applied, skipped, "restored");
}

fn create_view(
    node: &Node,
    parent: Option<(&ComponentId, &[SlotName], &SlotName)>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyView> {
    let kept = cx
        .default_global::<Registry>()
        .views
        .borrow()
        .get(&node.id)
        .cloned();
    let _group = kept.is_none().then(|| {
        tracing::info_span!(
            "component-create",
            node = %node.id,
            component = %node.component,
            extension = %node.extension,
        )
        .entered()
    });
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
            .current()
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
    let created = kept.is_none();
    let view = kept.unwrap_or_else(|| {
        let handle = (!is_frame_node(&node.id)).then(|| {
            let handle = cx.focus_handle();
            cx.default_global::<Registry>()
                .focus_handles
                .insert(node.id.clone(), handle.clone());
            handle
        });
        let view = (component.create)(node, slots.clone(), window, cx);
        let wrapped: AnyView = cx
            .new(|_| NodeView {
                node: node.id.clone(),
                handle,
                view,
            })
            .into();
        cx.default_global::<Registry>()
            .views
            .borrow_mut()
            .insert(node.id.clone(), wrapped.clone());
        wrapped
    });
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
    if created
        && let Some(axis) = match node.component.as_str() {
            "riso.row" => Some("row"),
            "riso.column" => Some("column"),
            _ => None,
        }
    {
        tracing::debug!(
            axis,
            children = slots.views("children").len(),
            "split created"
        );
    }
    if created && node.component.as_str() == "riso.tabs" {
        tracing::debug!(tabs = slots.views("children").len(), "tabs created");
    }
    Some(view)
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tracing::trace!("rendering shell");
        cx.defer_in(window, |shell, window, cx| shell.sync_focus(window, cx));
        window.set_rem_size(riso_ui::ui_font_size(cx));
        let action_handlers = cx
            .try_global::<Registry>()
            .map(|registry| registry.action_handlers.clone())
            .unwrap_or_default();
        let view = self.root.clone();
        let modal_layer = self.modal_layer.clone();
        let mut root = div();
        for attach in action_handlers {
            root = attach(root);
        }
        root.id("shell")
            .key_context("Shell")
            .track_focus(&self.handle)
            .on_action(|_: &ToggleZoom, window, cx| {
                layout_action(LayoutAction::ToggleZoom, None, window, cx);
            })
            .on_action(|_: &Hide, window, cx| {
                layout_action(LayoutAction::Hide, None, window, cx);
            })
            .on_action(|_: &SelectNextTab, window, cx| {
                layout_action(LayoutAction::SelectNextTab, None, window, cx);
            })
            .on_action(|_: &FocusLeft, window, cx| {
                layout_action(LayoutAction::FocusLeft, None, window, cx);
            })
            .on_action(|_: &FocusRight, window, cx| {
                layout_action(LayoutAction::FocusRight, None, window, cx);
            })
            .on_action(|_: &FocusUp, window, cx| {
                layout_action(LayoutAction::FocusUp, None, window, cx);
            })
            .on_action(|_: &FocusDown, window, cx| {
                layout_action(LayoutAction::FocusDown, None, window, cx);
            })
            .on_action(|_: &SelectPreviousTab, window, cx| {
                layout_action(LayoutAction::SelectPreviousTab, None, window, cx);
            })
            .relative()
            .size_full()
            .grid()
            .grid_cols(1)
            .grid_rows(1)
            .bg(riso_ui::palette(cx).background)
            .font(riso_ui::ui_font(cx))
            .on_drop::<DraggedNode>(|dragged, _, cx| {
                clear_drag_edges(cx);
                tracing::debug!(node = %dragged.node, "drop ignored");
            })
            .children(view)
            .child(self.toast_layer.clone())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .on_action(move |_: &Dismiss, window, cx| {
                        modal_layer.update(cx, |layer, cx| {
                            layer.hide_modal(riso_ui::ModalHideReason::Escape, window, cx);
                        });
                    })
                    .child(self.modal_layer.clone()),
            )
    }
}

/// Handles `riso::Quit` for the whole application: it quits.
pub fn handle_quit(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| {
        let _group = tracing::info_span!("app-quit").entered();
        cx.quit();
    });
}

/// Sets the application's menus.
pub fn set_menus(cx: &mut App, menus: Vec<gpui::Menu>) {
    let _group = tracing::info_span!("menus-set", count = menus.len()).entered();
    for menu in &menus {
        tracing::debug!(name = %menu.name, items = menu.items.len() as u64, "menu");
    }
    cx.set_menus(menus);
}
