use std::sync::atomic::{AtomicBool, Ordering};

use futures::executor::block_on;
use ksni::{
    Icon, ToolTip, TrayMethods as _,
    menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu},
};

use super::{Event, Events, TrayError, TrayIcon, TrayMenuItemSnapshot as Item};

pub(super) struct NativeTray(ksni::Handle<Notifier>);

struct Notifier {
    title: String,
    tooltip: String,
    icon: Icon,
    items: Vec<Item>,
    alternate_layout: bool,
    events: Events,
    watcher_online: AtomicBool,
}

impl NativeTray {
    pub(super) fn set(
        slot: &mut Option<Self>,
        product: &str,
        tooltip: &str,
        _icon: &TrayIcon,
        image: &image::DynamicImage,
        items: &[Item],
        events: Events,
    ) -> Result<(), TrayError> {
        let mut pixels = image.to_rgba8().into_raw();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.rotate_right(1);
        }
        let mut notifier = Notifier {
            title: product.into(),
            tooltip: tooltip.into(),
            icon: Icon {
                width: image.width() as i32,
                height: image.height() as i32,
                data: pixels,
            },
            items: items.to_vec(),
            alternate_layout: false,
            events,
            watcher_online: AtomicBool::new(true),
        };

        if let Some(tray) = slot.as_ref().filter(|tray| !tray.0.is_closed()) {
            return block_on(tray.0.update(|tray| {
                if !*tray.watcher_online.get_mut() {
                    return Err(TrayError::Unavailable);
                }
                notifier.alternate_layout = !tray.alternate_layout;
                *tray = notifier;
                Ok(())
            }))
            .ok_or(TrayError::Failed)?;
        }
        let handle = block_on(notifier.spawn()).map_err(|error| match error {
            ksni::Error::Dbus(_) | ksni::Error::Watcher(_) | ksni::Error::WontShow => {
                TrayError::Unavailable
            }
            _ => TrayError::Failed,
        })?;
        *slot = Some(Self(handle));
        Ok(())
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        block_on(self.0.shutdown());
    }
}

impl ksni::Tray for Notifier {
    fn id(&self) -> String {
        self.title.clone()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![self.icon.clone()]
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: self.tooltip.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.events.send(Event::Activate);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut menu = build_menu(&self.items, &self.events, &mut Vec::new());
        if !menu.is_empty() {
            // ksni only replaces native IDs when the tree structure changes.
            // Toggle a child on an invisible first entry so even identical
            // visible trees have different layouts.
            let hidden = || StandardItem {
                visible: false,
                enabled: false,
                ..Default::default()
            };
            let marker = if self.alternate_layout {
                SubMenu {
                    visible: false,
                    enabled: false,
                    submenu: vec![hidden().into()],
                    ..Default::default()
                }
                .into()
            } else {
                hidden().into()
            };
            menu.insert(0, marker);
        }
        menu
    }

    fn watcher_online(&self) {
        self.watcher_online.store(true, Ordering::Relaxed);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.watcher_online.store(false, Ordering::Relaxed);
        true
    }
}

fn build_menu(items: &[Item], events: &Events, path: &mut Vec<usize>) -> Vec<MenuItem<Notifier>> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            path.push(index);
            let item = match item {
                Item::Separator => MenuItem::Separator,
                Item::Submenu {
                    label,
                    disabled,
                    items,
                } => SubMenu {
                    label: label.replace("_", "__"),
                    enabled: !disabled,
                    submenu: build_menu(items, events, path),
                    ..Default::default()
                }
                .into(),
                Item::Action {
                    label,
                    disabled,
                    checked,
                    ..
                } => {
                    let events = events.clone();
                    let path = path.clone();
                    let activate = Box::new(move |_: &mut Notifier| {
                        events.send(Event::Menu(path.clone(), None))
                    });
                    if *checked {
                        CheckmarkItem {
                            label: label.replace("_", "__"),
                            enabled: !disabled,
                            checked: true,
                            activate,
                            ..Default::default()
                        }
                        .into()
                    } else {
                        StandardItem {
                            label: label.replace("_", "__"),
                            enabled: !disabled,
                            activate,
                            ..Default::default()
                        }
                        .into()
                    }
                }
            };
            path.pop();
            item
        })
        .collect()
}
