//! Component factories and the root view kept by the main window.

use gpui::{
    AnyView, App, AppContext, Context, Entity, Global, IntoElement, ParentElement, Render, Styled,
    Window, div,
};
use riso_layout::{ComponentId, Node, NodeId, Tree};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    rc::Rc,
};

type Factory = Rc<dyn Fn(&NodeId, &mut Window, &mut App) -> AnyView>;

#[derive(Default)]
struct Registry {
    components: BTreeMap<ComponentId, Factory>,
    tree: Tree,
}
impl Global for Registry {}

/// Registers the first factory for a valid component ID.
pub fn register(
    cx: &mut App,
    component: &str,
    create: impl Fn(&NodeId, &mut Window, &mut App) -> AnyView + 'static,
) {
    let _group = tracing::info_span!("component-register", component).entered();
    let component = match ComponentId::parse(component) {
        Ok(component) => component,
        Err(error) => {
            log::error!("invalid component ID {component:?}: {error}");
            return;
        }
    };
    match cx.default_global::<Registry>().components.entry(component) {
        Entry::Vacant(entry) => {
            entry.insert(Rc::new(create));
            tracing::debug!("registered");
        }
        Entry::Occupied(entry) => log::error!("component {} is already registered", entry.key()),
    }
}

/// Places a valid root, whether or not its component is registered yet.
pub fn place_root(cx: &mut App, node: &str, component: &str) {
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
    let _ = cx
        .default_global::<Registry>()
        .tree
        .place_root(Node { id, component });
}

/// The window's root view, retained for its lifetime.
pub struct Shell {
    root: Option<AnyView>,
}

/// Creates the root's component once, when the window opens.
pub fn new_shell(window: &mut Window, cx: &mut App) -> Entity<Shell> {
    let registry = cx.default_global::<Registry>();
    let root = registry.tree.root().cloned().map(|node| {
        let factory = registry.components.get(&node.component).cloned();
        (node, factory)
    });
    let root = root.and_then(|(node, factory)| {
        let _group =
            tracing::info_span!("component-create", node = %node.id, component = %node.component)
                .entered();
        match factory {
            Some(factory) => Some(factory(&node.id, window, cx)),
            None => {
                log::error!(
                    "node {} names unregistered component {}",
                    node.id,
                    node.component
                );
                None
            }
        }
    });
    cx.new(|_| Shell { root })
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
