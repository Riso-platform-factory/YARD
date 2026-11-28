use std::{
    collections::BTreeMap,
    fmt,
    io::Cursor,
    rc::Rc,
    sync::{Arc, Mutex},
};

use futures::{StreamExt as _, channel::mpsc};
use gpui::{Action, App, Global, MenuItem, SharedString};
use image::{ImageFormat, ImageReader, Limits};

use crate::Extension;

#[cfg(target_os = "macos")]
#[path = "tray/macos.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "tray/linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "tray/windows.rs"]
mod platform;

pub struct Tray {
    icon: TrayIcon,
    tooltip: Option<SharedString>,
    menu: Vec<MenuItem>,
}

impl Tray {
    pub fn new(icon: TrayIcon) -> Self {
        Self {
            icon,
            tooltip: None,
            menu: Vec::new(),
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn menu(mut self, menu: Vec<MenuItem>) -> Self {
        self.menu = menu;
        self
    }
}

pub struct TrayIcon {
    png: Vec<u8>,
    template: bool,
}

impl TrayIcon {
    pub fn png(bytes: impl AsRef<[u8]>) -> Self {
        Self {
            png: bytes.as_ref().to_vec(),
            template: false,
        }
    }

    pub fn template(mut self, template: bool) -> Self {
        self.template = template;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayError {
    AlreadyOwned,
    InvalidIcon,
    Unavailable,
    Failed,
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AlreadyOwned => "another extension owns the tray icon",
            Self::InvalidIcon => "tray icon is not a valid PNG",
            Self::Unavailable => "no tray on this desktop",
            Self::Failed => "tray icon failed",
        })
    }
}

impl std::error::Error for TrayError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayMenuItemSnapshot {
    Action {
        label: String,
        action_name: String,
        checked: bool,
        disabled: bool,
    },
    Separator,
    Submenu {
        label: String,
        disabled: bool,
        items: Vec<TrayMenuItemSnapshot>,
    },
}

#[derive(Clone)]
pub struct TrayBackend {
    kind: BackendKind,
}

#[derive(Clone)]
enum BackendKind {
    System(String),
    Memory(Arc<Mutex<Memory>>),
}

#[derive(Default)]
struct Memory {
    current: Option<MemoryItem>,
    next_error: Option<TrayError>,
}

struct MemoryItem {
    size: (u32, u32),
    template: bool,
    tooltip: Option<String>,
    menu: Vec<TrayMenuItemSnapshot>,
    events: Events,
}

#[derive(Clone)]
struct Events {
    revision: u64,
    sender: mpsc::UnboundedSender<(u64, Event)>,
}

enum Event {
    Activate,
    Menu(Vec<usize>, Option<gpui::AnyWindowHandle>),
    #[cfg(windows)]
    OpenMenu {
        tray: isize,
        active: isize,
        position: (i32, i32),
    },
}

impl Events {
    fn send(&self, event: Event) {
        let _ = self.sender.unbounded_send((self.revision, event));
    }
}

impl TrayBackend {
    pub fn system(product_name: &str) -> Self {
        Self {
            kind: BackendKind::System(product_name.into()),
        }
    }

    pub fn memory() -> Self {
        Self {
            kind: BackendKind::Memory(Arc::default()),
        }
    }

    pub fn fail_next(&self, error: TrayError) {
        if let BackendKind::Memory(memory) = &self.kind {
            memory.lock().unwrap().next_error = Some(error);
        }
    }

    pub fn simulate_activate(&self) {
        self.with_item(|item| item.events.send(Event::Activate));
    }

    /// Selects a path of zero-based indices into the recorded menu.
    pub fn simulate_menu(&self, path: &[usize]) {
        self.with_item(|item| item.events.send(Event::Menu(path.to_vec(), None)));
    }

    /// Returns the current icon's (width, height) in pixels.
    pub fn icon_size(&self) -> Option<(u32, u32)> {
        self.with_item(|item| item.size)
    }

    pub fn icon_template(&self) -> Option<bool> {
        self.with_item(|item| item.template)
    }

    pub fn tooltip(&self) -> Option<String> {
        self.with_item(|item| item.tooltip.clone()).flatten()
    }

    pub fn menu_snapshot(&self) -> Vec<TrayMenuItemSnapshot> {
        self.with_item(|item| item.menu.clone()).unwrap_or_default()
    }

    fn with_item<T>(&self, read: impl FnOnce(&MemoryItem) -> T) -> Option<T> {
        match &self.kind {
            BackendKind::Memory(memory) => memory.lock().unwrap().current.as_ref().map(read),
            BackendKind::System(_) => None,
        }
    }
}

type Handler = Rc<dyn Fn(&mut App)>;
type MenuActions = BTreeMap<Vec<usize>, Box<dyn Action>>;

#[derive(Default)]
struct TrayState {
    backend: Option<TrayBackend>,
    sender: Option<mpsc::UnboundedSender<(u64, Event)>>,
    missing_logged: bool,
    shutting_down: bool,
    revision: u64,
    shown: Option<Shown>,
    handlers: BTreeMap<String, Handler>,
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    native: Option<platform::NativeTray>,
}

struct Shown {
    owner: String,
    revision: u64,
    actions: MenuActions,
}

impl Global for TrayState {}

impl TrayState {
    fn clear(&mut self) {
        if self.shown.take().is_none() {
            return;
        }
        if let Some(TrayBackend {
            kind: BackendKind::Memory(memory),
        }) = &self.backend
        {
            memory.lock().unwrap().current = None;
        }
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        drop(self.native.take());
    }

    fn show(&mut self, owner: &str, tray: Tray) -> Result<(), TrayError> {
        if self.shutting_down {
            return Err(TrayError::Unavailable);
        }
        let Some(backend) = &self.backend else {
            if !std::mem::replace(&mut self.missing_logged, true) {
                log::error!("tray backend has not been set");
            }
            return Err(TrayError::Unavailable);
        };

        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.owner != owner)
        {
            return Err(TrayError::AlreadyOwned);
        }

        let mut reader = ImageReader::with_format(Cursor::new(&tray.icon.png), ImageFormat::Png);
        let mut limits = Limits::default();
        limits.max_image_width = Some(256);
        limits.max_image_height = Some(256);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| TrayError::InvalidIcon)?;
        let mut actions = BTreeMap::new();
        let menu = snapshot(tray.menu, &mut Vec::new(), true, &mut actions);
        self.revision += 1;
        let events = Events {
            revision: self.revision,
            sender: self
                .sender
                .as_ref()
                .expect("installed tray receiver")
                .clone(),
        };
        match &backend.kind {
            BackendKind::Memory(memory) => {
                let mut memory = memory.lock().unwrap();
                if let Some(error) = memory.next_error.take() {
                    return Err(error);
                }
                memory.current = Some(MemoryItem {
                    size: (image.width(), image.height()),
                    template: tray.icon.template,
                    tooltip: tray.tooltip.map(|text| text.to_string()),
                    menu,
                    events,
                });
            }
            BackendKind::System(product) => {
                let tooltip = tray.tooltip.as_deref().unwrap_or(product);
                #[cfg(any(target_os = "macos", target_os = "linux", windows))]
                platform::NativeTray::set(
                    &mut self.native,
                    product,
                    tooltip,
                    &tray.icon,
                    &image,
                    &menu,
                    events,
                )?;
                #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
                {
                    let _ = tooltip;
                    return Err(TrayError::Unavailable);
                }
            }
        }
        self.shown = Some(Shown {
            owner: owner.into(),
            revision: self.revision,
            actions,
        });
        Ok(())
    }
}

impl Drop for TrayState {
    fn drop(&mut self) {
        self.clear();
    }
}

pub fn set_tray_backend(cx: &mut App, backend: TrayBackend) {
    let state = cx.default_global::<TrayState>();
    if state.backend.is_some() {
        log::error!("tray backend was already set");
        return;
    }
    let (sender, mut receiver) = mpsc::unbounded();
    state.backend = Some(backend);
    state.sender = Some(sender);
    cx.spawn(async move |cx| {
        while let Some((revision, event)) = receiver.next().await {
            #[cfg(windows)]
            if let Event::OpenMenu {
                tray,
                active,
                position,
            } = event
            {
                let target = cx.update(|cx| platform::menu_target(active, cx));
                platform::post_menu(tray, revision, target, position);
                continue;
            }
            cx.update(|cx| deliver(revision, event, cx));
        }
    })
    .detach();
    cx.on_app_quit(|cx| {
        let state = cx.default_global::<TrayState>();
        state.shutting_down = true;
        state.clear();
        async {}
    })
    .detach();
}

impl Extension {
    pub fn set_tray(&self, tray: Tray, cx: &mut App) -> Result<(), TrayError> {
        let _group = tracing::info_span!(
            "tray-set",
            extension = self.name.as_str(),
            items = tray.menu.len()
        )
        .entered();
        let state = cx.default_global::<TrayState>();
        let updating = state.shown.is_some();
        let result = state.show(&self.name, tray);
        match result {
            Ok(()) if updating => tracing::debug!("updated"),
            Ok(()) => tracing::debug!("shown"),
            Err(error @ (TrayError::AlreadyOwned | TrayError::InvalidIcon)) => {
                tracing::info!(reason = %error, "refused");
            }
            Err(error @ (TrayError::Unavailable | TrayError::Failed)) => {
                tracing::warn!(reason = %error, "tray setup failed");
                log::warn!("tray setup for {} failed: {error}", self.name);
            }
        }
        result
    }

    pub fn remove_tray(&self, cx: &mut App) {
        let state = cx.default_global::<TrayState>();
        if state
            .shown
            .as_ref()
            .is_some_and(|shown| shown.owner == self.name)
        {
            let _group =
                tracing::info_span!("tray-remove", extension = self.name.as_str()).entered();
            state.clear();
            tracing::debug!("removed");
        }
    }

    pub fn on_tray_activate(&self, cx: &mut App, handler: impl Fn(&mut App) + 'static) {
        let previous = cx
            .default_global::<TrayState>()
            .handlers
            .insert(self.name.clone(), Rc::new(handler));
        if previous.is_some() {
            tracing::debug!(
                extension = self.name.as_str(),
                "replaced tray activation handler"
            );
        }
    }
}

fn snapshot(
    items: Vec<MenuItem>,
    path: &mut Vec<usize>,
    enabled: bool,
    actions: &mut MenuActions,
) -> Vec<TrayMenuItemSnapshot> {
    let mut menu = Vec::new();
    for item in items {
        path.push(menu.len());
        let entry = match item {
            MenuItem::Action {
                name,
                action,
                checked,
                disabled,
                ..
            } => {
                let action_name = action.name().to_string();
                if enabled && !disabled {
                    actions.insert(path.clone(), action);
                }
                Some(TrayMenuItemSnapshot::Action {
                    label: name.to_string(),
                    action_name,
                    checked,
                    disabled,
                })
            }
            MenuItem::Separator => Some(TrayMenuItemSnapshot::Separator),
            MenuItem::Submenu(submenu) => Some(TrayMenuItemSnapshot::Submenu {
                label: submenu.name.to_string(),
                disabled: submenu.disabled,
                items: snapshot(submenu.items, path, enabled && !submenu.disabled, actions),
            }),
            MenuItem::SystemMenu(menu) => {
                tracing::info!(label = menu.name.as_ref(), "unsupported tray menu item");
                None
            }
        };
        path.pop();
        menu.extend(entry);
    }
    menu
}

fn deliver(revision: u64, event: Event, cx: &mut App) {
    let state = cx.global::<TrayState>();
    let Some(shown) = state
        .shown
        .as_ref()
        .filter(|shown| shown.revision == revision)
    else {
        return;
    };
    let owner = shown.owner.clone();
    match event {
        Event::Activate => {
            let handler = state.handlers.get(&owner).cloned();
            let _group = tracing::info_span!("tray-activate", extension = owner.as_str()).entered();
            if let Some(handler) = handler {
                handler(cx);
                tracing::debug!("handled");
            } else {
                tracing::debug!("no handler");
            }
        }
        Event::Menu(path, preferred_window) => {
            let Some(action) = shown.actions.get(&path).map(|action| action.boxed_clone()) else {
                return;
            };
            let _group = tracing::info_span!(
                "tray-menu-action",
                extension = owner.as_str(),
                action = action.name()
            )
            .entered();
            let (window, target) = if let Some(window) = preferred_window
                .filter(|window| cx.windows().contains(window))
                .or_else(|| cx.active_window())
            {
                (Some(window), "window")
            } else {
                (cx.windows().into_iter().next(), "first window")
            };
            if let Some(window) = window
                && window
                    .update(cx, |_, window, cx| {
                        window.dispatch_action(action.boxed_clone(), cx)
                    })
                    .is_ok()
            {
                tracing::debug!(target, "dispatched");
                return;
            }
            cx.dispatch_action(action.as_ref());
            tracing::debug!(target = "application", "dispatched");
        }
        #[cfg(windows)]
        Event::OpenMenu { .. } => unreachable!("handled before dispatch"),
    }
}
