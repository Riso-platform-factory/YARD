use std::cell::RefCell;

use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, sel,
};
use objc2_app_kit::{
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSObject, NSSize, NSString};

use super::{Event, Events, TrayError, TrayIcon, TrayMenuItemSnapshot as Item};

struct TargetState {
    events: Events,
    paths: RefCell<Vec<Vec<usize>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetState]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(yardTrayActivate:))]
        fn activate(&self, _sender: &objc2::runtime::AnyObject) {
            self.ivars().events.send(Event::Activate);
        }

        #[unsafe(method(yardTraySelect:))]
        fn select(&self, item: &NSMenuItem) {
            if let Some(path) = self.ivars().paths.borrow().get(item.tag() as usize) {
                self.ivars().events.send(Event::Menu(path.clone(), None));
            }
        }
    }
);

pub(super) struct NativeTray {
    bar: Retained<NSStatusBar>,
    item: Retained<NSStatusItem>,
    _target: Option<Retained<TrayTarget>>,
}

impl NativeTray {
    pub(super) fn set(
        slot: &mut Option<Self>,
        _product: &str,
        tooltip: &str,
        icon: &TrayIcon,
        decoded: &image::DynamicImage,
        items: &[Item],
        events: Events,
    ) -> Result<(), TrayError> {
        let mtm = MainThreadMarker::new().ok_or(TrayError::Failed)?;
        let data = NSData::with_bytes(&icon.png);
        let image = NSImage::initWithData(NSImage::alloc(), &data).ok_or(TrayError::Failed)?;
        let width = 18.0 * f64::from(decoded.width()) / f64::from(decoded.height());
        image.setSize(NSSize::new(width, 18.0));
        image.setTemplate(icon.template);

        let allocated = TrayTarget::alloc(mtm).set_ivars(TargetState {
            events,
            paths: RefCell::default(),
        });
        // SAFETY: NSObject's init initializes the superclass of this allocated target.
        let target: Retained<TrayTarget> = unsafe { msg_send![super(allocated), init] };
        let menu =
            (!items.is_empty()).then(|| build_menu("", items, &target, &mut Vec::new(), mtm));

        let new_item;
        let tray = if let Some(tray) = slot.as_mut() {
            tray
        } else {
            let bar = NSStatusBar::systemStatusBar();
            let item = bar.statusItemWithLength(NSVariableStatusItemLength);
            new_item = Self {
                bar,
                item,
                _target: None,
            };
            slot.insert(new_item)
        };
        let Some(button) = tray.item.button(mtm) else {
            if tray._target.is_none() {
                drop(slot.take());
            }
            return Err(TrayError::Failed);
        };
        button.setImage(Some(&image));
        button.setToolTip(Some(&NSString::from_str(tooltip)));
        tray.item.setMenu(menu.as_deref());
        // SAFETY: TrayTarget implements this selector and is retained for as
        // long as the button and menu point to it.
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(yardTrayActivate:)));
        }
        tray._target = Some(target);
        Ok(())
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.bar.removeStatusItem(&self.item);
    }
}

fn build_menu(
    title: &str,
    items: &[Item],
    target: &TrayTarget,
    path: &mut Vec<usize>,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    menu.setAutoenablesItems(false);
    for (index, item) in items.iter().enumerate() {
        path.push(index);
        match item {
            Item::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Item::Action {
                label,
                checked,
                disabled,
                ..
            } => {
                // SAFETY: The target implements the selector with an NSMenuItem sender.
                let entry = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label),
                        Some(sel!(yardTraySelect:)),
                        &NSString::from_str(""),
                    )
                };
                let mut paths = target.ivars().paths.borrow_mut();
                entry.setTag(paths.len() as isize);
                paths.push(path.clone());
                // SAFETY: NativeTray retains the target until the menu is replaced or removed.
                unsafe { entry.setTarget(Some(target)) };
                menu.addItem(&entry);
                entry.setEnabled(!disabled);
                entry.setState(isize::from(*checked));
            }
            Item::Submenu {
                label,
                disabled,
                items,
            } => {
                // SAFETY: The submenu's parent item has no selector.
                let entry = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(label),
                        None,
                        &NSString::from_str(""),
                    )
                };
                menu.addItem(&entry);
                entry.setSubmenu(Some(&build_menu(label, items, target, path, mtm)));
                entry.setEnabled(!disabled);
            }
        }
        path.pop();
    }
    menu
}
