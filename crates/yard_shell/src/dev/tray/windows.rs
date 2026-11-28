use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::{Mutex, OnceLock},
};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
            DeleteObject, HBITMAP, HGDIOBJ,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::GetActiveWindow,
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NOTIFY_ICON_MESSAGE, NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyIcon, DestroyMenu, DestroyWindow, FindWindowW, GetCursorPos, HICON, HMENU,
                ICONINFO, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
                MF_STRING, MF_UNCHECKED, PostMessageW, RegisterClassW, RegisterWindowMessageW,
                SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
                WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{Event, Events, TrayError, TrayIcon, TrayMenuItemSnapshot as Item};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const SHOW_MENU_MESSAGE: u32 = WM_APP + 2;
const CLASS: PCWSTR = w!("YardShellTrayWindow");
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static WINDOW_CLASS_REGISTERED: Mutex<bool> = Mutex::new(false);

thread_local! {
    static ITEMS: RefCell<BTreeMap<isize, Rc<Configuration>>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) struct NativeTray(HWND);

struct Configuration {
    icon: RefCell<Icon>,
    image: image::DynamicImage,
    tooltip: [u16; 128],
    popup: Option<Popup>,
    pending_popup: Cell<Option<PendingPopup>>,
    events: Events,
}

#[derive(Clone, Copy)]
struct PendingPopup {
    target: Option<gpui::AnyWindowHandle>,
    position: (i32, i32),
}

struct Popup {
    menu: Menu,
    paths: Vec<Vec<usize>>,
}

impl NativeTray {
    pub(super) fn set(
        slot: &mut Option<Self>,
        _product: &str,
        tooltip: &str,
        _icon: &TrayIcon,
        image: &image::DynamicImage,
        items: &[Item],
        events: Events,
    ) -> Result<(), TrayError> {
        let popup = if items.is_empty() {
            None
        } else {
            let mut paths = Vec::new();
            let menu = Menu::new(items, &mut Vec::new(), &mut paths)?;
            Some(Popup { menu, paths })
        };
        let new_tray = if slot.is_none() {
            Some(Self::create_window()?)
        } else {
            None
        };
        let tray = slot.as_ref().or(new_tray.as_ref()).expect("tray window");
        let operation = if new_tray.is_some() {
            NIM_ADD
        } else {
            NIM_MODIFY
        };
        let icon = Icon::new(image, tray.0)?;
        let configuration = Configuration {
            icon: RefCell::new(icon),
            image: image.clone(),
            tooltip: tooltip_buffer(tooltip),
            popup,
            pending_popup: Cell::new(None),
            events,
        };
        let icon = configuration.icon.borrow().0;
        if !configuration.notify(tray.0, operation, icon)
            && (operation == NIM_ADD || !configuration.notify(tray.0, NIM_ADD, icon))
        {
            return Err(TrayError::Failed);
        }
        ITEMS.with(|items| {
            items
                .borrow_mut()
                .insert(tray.0.0 as isize, Rc::new(configuration))
        });
        if new_tray.is_some() {
            *slot = new_tray;
        }
        Ok(())
    }

    fn create_window() -> Result<Self, TrayError> {
        // SAFETY: The class and broadcast message are registered with static
        // UTF-16 strings. The hidden top-level window belongs to this UI thread.
        unsafe {
            if TASKBAR_CREATED.get().is_none() {
                let message = RegisterWindowMessageW(w!("TaskbarCreated"));
                if message == 0 {
                    return Err(TrayError::Failed);
                }
                let _ = TASKBAR_CREATED.set(message);
            }
            let module = GetModuleHandleW(PCWSTR::null()).map_err(|_| TrayError::Failed)?;
            let instance = HINSTANCE(module.0);
            {
                let mut registered = WINDOW_CLASS_REGISTERED.lock().unwrap();
                if !*registered {
                    let atom = RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: instance,
                        lpszClassName: CLASS,
                        ..Default::default()
                    });
                    if atom == 0 {
                        return Err(TrayError::Failed);
                    }
                    *registered = true;
                }
            }
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS,
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map(Self)
            .map_err(|_| TrayError::Failed)
        }
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        let configuration = ITEMS.with(|items| items.borrow_mut().remove(&(self.0.0 as isize)));
        if let Some(configuration) = configuration {
            let icon = configuration.icon.borrow().0;
            configuration.notify(self.0, NIM_DELETE, icon);
        }
        // SAFETY: This window was created on the current thread and is owned by self.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

impl Configuration {
    fn notify(&self, hwnd: HWND, operation: NOTIFY_ICON_MESSAGE, icon: HICON) -> bool {
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: icon,
            szTip: self.tooltip,
            ..Default::default()
        };
        // SAFETY: The structure and icon remain valid for the call.
        unsafe { Shell_NotifyIconW(operation, &data).as_bool() }
    }

    fn show_menu(&self, hwnd: HWND, (x, y): (i32, i32)) -> Option<Vec<usize>> {
        let popup = self.popup.as_ref()?;
        // SAFETY: We hold the popup and its native menu throughout the modal
        // call. The tracking flags return the command ID without WM_COMMAND.
        let command = unsafe {
            let _ = SetForegroundWindow(hwnd);
            let command = TrackPopupMenuEx(
                popup.menu.0,
                (TPM_RIGHTBUTTON | TPM_NONOTIFY | TPM_RETURNCMD).0,
                x,
                y,
                hwnd,
                None,
            )
            .0;
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            command
        };
        if command > 0 {
            return popup.paths.get(command as usize - 1).cloned();
        }
        None
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == SHOW_MENU_MESSAGE {
        let revision = u64::from(wparam.0 as u32) | (u64::from(lparam.0 as u32) << 32);
        if let Some(configuration) = configuration_for_window(hwnd)
            && configuration.events.revision == revision
            && let Some(pending) = configuration.pending_popup.take()
            && let Some(path) = configuration.show_menu(hwnd, pending.position)
            // The modal loop can run GPUI work that replaces or removes the tray.
            && let Some(current) = configuration_for_window(hwnd)
            && Rc::ptr_eq(&current, &configuration)
        {
            configuration.events.send(Event::Menu(path, pending.target));
        }
        return LRESULT(0);
    }
    if message == TRAY_MESSAGE || TASKBAR_CREATED.get() == Some(&message) {
        if let Some(configuration) = configuration_for_window(hwnd) {
            if message == TRAY_MESSAGE {
                match lparam.0 as u32 {
                    WM_LBUTTONUP => configuration.events.send(Event::Activate),
                    WM_RBUTTONUP if configuration.popup.is_some() => {
                        let mut cursor = POINT::default();
                        // SAFETY: The cursor output is valid and the active window
                        // lookup is scoped to the current UI thread.
                        if unsafe { GetCursorPos(&mut cursor) }.is_ok() {
                            let active = unsafe { GetActiveWindow() }.0 as isize;
                            configuration.events.send(Event::OpenMenu {
                                tray: hwnd.0 as isize,
                                active,
                                position: (cursor.x, cursor.y),
                            });
                        }
                    }
                    _ => {}
                }
            } else {
                match Icon::new(&configuration.image, hwnd) {
                    Ok(icon) if configuration.notify(hwnd, NIM_ADD, icon.0) => {
                        configuration.icon.replace(icon);
                    }
                    _ => log::warn!("tray icon could not be restored after Explorer restarted"),
                }
            }
        }
        return LRESULT(0);
    }
    // SAFETY: These parameters come unchanged from the system.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub(super) fn menu_target(active: isize, cx: &gpui::App) -> Option<gpui::AnyWindowHandle> {
    // GPUI maps the current thread's active HWND to its window handle.
    // SAFETY: Both lookups run on the UI thread before the popup takes focus.
    if active != 0 && unsafe { GetActiveWindow() }.0 as isize == active {
        cx.active_window()
    } else {
        None
    }
}

fn configuration_for_window(hwnd: HWND) -> Option<Rc<Configuration>> {
    ITEMS.with(|items| items.borrow().get(&(hwnd.0 as isize)).cloned())
}

pub(super) fn post_menu(
    tray: isize,
    revision: u64,
    target: Option<gpui::AnyWindowHandle>,
    position: (i32, i32),
) {
    let hwnd = HWND(tray as *mut _);
    if let Some(configuration) = configuration_for_window(hwnd)
        && configuration.events.revision == revision
    {
        if configuration
            .pending_popup
            .replace(Some(PendingPopup { target, position }))
            .is_some()
        {
            return;
        }
        // Let the GPUI runnable return before entering the popup's modal loop,
        // so the Windows dispatcher can post new foreground wake messages.
        // Carry the revision in two parts so a queued message can be rejected
        // after an update, including on Windows with 32-bit message parameters.
        // SAFETY: The HWND is the current tray window and both parameters are integers.
        if unsafe {
            PostMessageW(
                Some(hwnd),
                SHOW_MENU_MESSAGE,
                WPARAM(revision as u32 as usize),
                LPARAM((revision >> 32) as u32 as isize),
            )
        }
        .is_err()
        {
            configuration.pending_popup.take();
        }
    }
}

fn tooltip_buffer(text: &str) -> [u16; 128] {
    let mut result = [0; 128];
    let mut position = 0;
    for character in text.chars().take_while(|character| *character != '\0') {
        let end = position + character.len_utf16();
        if end >= result.len() {
            break;
        }
        character.encode_utf16(&mut result[position..end]);
        position = end;
    }
    result
}

struct Menu(HMENU);

impl Menu {
    fn new(
        items: &[Item],
        path: &mut Vec<usize>,
        paths: &mut Vec<Vec<usize>>,
    ) -> Result<Self, TrayError> {
        // SAFETY: The returned menu is owned by the wrapper.
        let menu = Self(unsafe { CreatePopupMenu() }.map_err(|_| TrayError::Failed)?);
        for (index, item) in items.iter().enumerate() {
            path.push(index);
            match item {
                Item::Separator => menu.append(MF_SEPARATOR, 0, "")?,
                Item::Action {
                    label,
                    disabled,
                    checked,
                    ..
                } => {
                    paths.push(path.clone());
                    let check = if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    menu.append(MF_STRING | enabled(*disabled) | check, paths.len(), label)?;
                }
                Item::Submenu {
                    label,
                    disabled,
                    items,
                } => {
                    let child = Self::new(items, path, paths)?;
                    menu.append(MF_POPUP | enabled(*disabled), child.0.0 as usize, label)?;
                    // DestroyMenu on the parent now destroys the submenu as well.
                    std::mem::forget(child);
                }
            }
            path.pop();
        }
        Ok(menu)
    }

    fn append(&self, flags: MENU_ITEM_FLAGS, id: usize, label: &str) -> Result<(), TrayError> {
        // Win32 interprets an ampersand as a mnemonic marker.
        let label: Vec<u16> = label.replace('&', "&&").encode_utf16().chain([0]).collect();
        // SAFETY: label is null-terminated and remains valid through the call.
        unsafe { AppendMenuW(self.0, flags, id, PCWSTR(label.as_ptr())) }
            .map_err(|_| TrayError::Failed)
    }
}

fn enabled(disabled: bool) -> MENU_ITEM_FLAGS {
    if disabled {
        MF_GRAYED
    } else {
        MENU_ITEM_FLAGS(0)
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: The menu is owned by this wrapper.
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: This handle belongs to the wrapper and is not selected into a DC.
        let _ = unsafe { DeleteObject(HGDIOBJ(self.0.0)) };
    }
}

struct Icon(HICON);

impl Icon {
    fn new(image: &image::DynamicImage, hwnd: HWND) -> Result<Self, TrayError> {
        // SAFETY: The taskbar class is a static string and the HWND is owned by the tray.
        let size = unsafe {
            let taskbar = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or(hwnd);
            let mut dpi = GetDpiForWindow(taskbar);
            if dpi == 0 {
                dpi = GetDpiForWindow(hwnd);
            }
            GetSystemMetricsForDpi(SM_CXSMICON, dpi).min(GetSystemMetricsForDpi(SM_CYSMICON, dpi))
        };
        if size <= 0 {
            return Err(TrayError::Failed);
        }
        let (width, height) = (size, size);
        let size = size as u32;
        let scaled = image
            .resize(size, size, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let mut pixels = image::RgbaImage::new(size, size);
        image::imageops::replace(
            &mut pixels,
            &scaled,
            i64::from((size - scaled.width()) / 2),
            i64::from((size - scaled.height()) / 2),
        );
        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: The header describes a 32-bit top-down bitmap with a
        // width * 4 byte stride. CreateDIBSection owns the returned pixel storage.
        let color = Bitmap(
            unsafe { CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }
                .map_err(|_| TrayError::Failed)?,
        );
        if bits.is_null() {
            return Err(TrayError::Failed);
        }
        // SAFETY: The destination contains the same number of bytes as the resized RGBA image.
        let bgra =
            unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), pixels.as_raw().len()) };
        for (out, pixel) in bgra.as_chunks_mut::<4>().0.iter_mut().zip(pixels.pixels()) {
            let [red, green, blue, alpha] = pixel.0;
            let premultiply =
                |component| ((u16::from(component) * u16::from(alpha) + 127) / 255) as u8;
            out.copy_from_slice(&[
                premultiply(blue),
                premultiply(green),
                premultiply(red),
                alpha,
            ]);
        }
        let mask_stride = (width as usize).div_ceil(16) * 2;
        let mask_data = vec![0u8; mask_stride * height as usize];
        // SAFETY: The one-bit mask buffer is aligned by scan line to whole words
        // and the call copies it into the bitmap.
        let mask = unsafe { CreateBitmap(width, height, 1, 1, Some(mask_data.as_ptr().cast())) };
        if mask.is_invalid() {
            return Err(TrayError::Failed);
        }
        let mask = Bitmap(mask);
        let info = ICONINFO {
            fIcon: true.into(),
            hbmMask: mask.0,
            hbmColor: color.0,
            ..Default::default()
        };
        // SAFETY: Both bitmap handles are valid and CreateIconIndirect copies them.
        unsafe { CreateIconIndirect(&info) }
            .map(Self)
            .map_err(|_| TrayError::Failed)
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: This icon was created by CreateIconIndirect and is owned by the wrapper.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}
