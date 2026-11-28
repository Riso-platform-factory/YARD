use crate::Extension;
use gpui::{App, Global};
use std::{
    cell::RefCell,
    collections::HashMap,
    fmt,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
};

#[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::path::Path;

#[cfg(any(test, target_os = "linux"))]
mod linux;
#[cfg(test)]
use linux::linux_autostart;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartStatus {
    Enabled,
    Disabled,
    NeedsApproval,
    NotSupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartError {
    NotSupported,
    NeedsApproval,
    Failed,
}

impl fmt::Display for AutostartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotSupported => "launch at login is not supported here",
            Self::NeedsApproval => {
                "launch at login needs the user's approval in the system settings"
            }
            Self::Failed => "launch at login failed",
        })
    }
}

impl std::error::Error for AutostartError {}

#[derive(Clone)]
pub struct AutostartBackend {
    kind: BackendKind,
}

#[derive(Clone)]
enum BackendKind {
    System(SystemBackend),
    Memory(Arc<Mutex<Memory>>),
}

#[derive(Clone)]
struct SystemBackend {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    identifier: String,
    #[cfg(target_os = "linux")]
    product_name: String,
}

struct Memory {
    status: AutostartStatus,
    failure: Option<AutostartError>,
}

type Operation = Result<AutostartStatus, OperationError>;

struct OperationError {
    error: AutostartError,
    system_status: Option<i64>,
}

impl From<AutostartError> for OperationError {
    fn from(error: AutostartError) -> Self {
        Self {
            error,
            system_status: None,
        }
    }
}

impl OperationError {
    #[cfg(any(test, target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn io(error: std::io::Error) -> Self {
        Self {
            error: AutostartError::Failed,
            system_status: error.raw_os_error().map(i64::from),
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn system(status: impl Into<i64>) -> Self {
        Self {
            error: AutostartError::Failed,
            system_status: Some(status.into()),
        }
    }
}

impl AutostartBackend {
    pub fn system(_identifier: &str, _product_name: &str) -> Self {
        Self {
            kind: BackendKind::System(SystemBackend {
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                identifier: _identifier.into(),
                #[cfg(target_os = "linux")]
                product_name: _product_name.into(),
            }),
        }
    }

    pub fn memory() -> Self {
        Self {
            kind: BackendKind::Memory(Arc::new(Mutex::new(Memory {
                status: AutostartStatus::Disabled,
                failure: None,
            }))),
        }
    }

    pub fn set_status(&self, status: AutostartStatus) {
        if let BackendKind::Memory(memory) = &self.kind {
            memory.lock().unwrap().status = status;
        }
    }

    pub fn fail_next(&self, error: AutostartError) {
        if let BackendKind::Memory(memory) = &self.kind {
            memory.lock().unwrap().failure = Some(error);
        }
    }

    fn operate(&self, enabled: Option<bool>) -> Operation {
        match &self.kind {
            BackendKind::System(system) => system.operate(enabled),
            BackendKind::Memory(memory) => {
                let mut memory = memory.lock().unwrap();
                if let Some(error) = memory.failure.take() {
                    return Err(error.into());
                }
                if let Some(enabled) = enabled {
                    memory.status = match (memory.status, enabled) {
                        (AutostartStatus::NotSupported, _) => {
                            return Err(AutostartError::NotSupported.into());
                        }
                        (AutostartStatus::NeedsApproval, true) => {
                            return Err(AutostartError::NeedsApproval.into());
                        }
                        (_, true) => AutostartStatus::Enabled,
                        (_, false) => AutostartStatus::Disabled,
                    };
                }
                Ok(memory.status)
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
impl SystemBackend {
    fn operate(&self, enabled: Option<bool>) -> Operation {
        match enabled {
            Some(_) => Err(AutostartError::NotSupported.into()),
            None => Ok(AutostartStatus::NotSupported),
        }
    }
}

struct InstalledBackend(AutostartBackend);

impl Global for InstalledBackend {}

pub fn set_autostart_backend(cx: &mut App, backend: AutostartBackend) {
    if cx.has_global::<InstalledBackend>() {
        log::error!("autostart backend was already set");
        return;
    }
    cx.set_global(InstalledBackend(backend));
}

impl Extension {
    pub fn launch_at_login(&self, cx: &App) -> Result<AutostartStatus, AutostartError> {
        let _group =
            tracing::info_span!("autostart-status", extension = self.name.as_str()).entered();
        self.autostart(None, cx)
    }

    pub fn set_launch_at_login(
        &self,
        enabled: bool,
        cx: &App,
    ) -> Result<AutostartStatus, AutostartError> {
        let _group =
            tracing::info_span!("autostart-set", extension = self.name.as_str(), enabled).entered();
        self.autostart(Some(enabled), cx)
    }

    fn autostart(
        &self,
        enabled: Option<bool>,
        cx: &App,
    ) -> Result<AutostartStatus, AutostartError> {
        let Some(InstalledBackend(backend)) = cx.try_global::<InstalledBackend>() else {
            tracing::warn!("autostart backend has not been set");
            missing_backend(cx);
            return Err(AutostartError::Failed);
        };
        match backend.operate(enabled) {
            Ok(status) => {
                if enabled.is_some() {
                    tracing::debug!(?status, "updated");
                } else {
                    tracing::debug!(?status, "checked");
                }
                Ok(status)
            }
            Err(OperationError {
                error,
                system_status,
            }) => {
                match error {
                    AutostartError::Failed => {
                        tracing::warn!(reason = %error, system_status, "launch at login failed");
                        if let Some(status) = system_status {
                            log::warn!("{error} (system status {status})");
                        } else {
                            log::warn!("{error}");
                        }
                    }
                    _ if enabled.is_some() => tracing::info!(reason = %error, "refused"),
                    _ => {}
                }
                Err(error)
            }
        }
    }
}

fn missing_backend(cx: &App) {
    thread_local! {
        static REPORTED: RefCell<HashMap<*const App, Weak<()>>> = RefCell::new(HashMap::new());
    }
    let first = REPORTED.with_borrow_mut(|reported| {
        reported.retain(|_, marker| marker.strong_count() != 0);
        let std::collections::hash_map::Entry::Vacant(entry) = reported.entry(cx) else {
            return false;
        };
        // The operations have only &App. Keep the marker in an app-owned observer
        // so a later app can reuse the same address without inheriting the report.
        let marker = Rc::new(());
        entry.insert(Rc::downgrade(&marker));
        cx.on_app_lifecycle(move |_, _| {
            let _marker = &marker;
        })
        .detach();
        true
    });
    if first {
        log::error!("autostart backend has not been set");
    }
}

#[cfg(any(test, target_os = "linux"))]
fn desktop_entry(product_name: &str, exe: &Path) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    let without_leading = product_name.trim_start_matches(' ');
    let trimmed = without_leading.trim_end_matches(' ');
    let leading = product_name.len() - without_leading.len();
    let trailing = without_leading.len() - trimmed.len();
    let name = escape(trimmed);
    let name = format!("{}{name}{}", "\\s".repeat(leading), "\\s".repeat(trailing));
    let command = exe
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    let command = escape(&command);
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{command}\"\nX-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(any(test, target_os = "linux"))]
fn desktop_entry_enabled(text: &str) -> bool {
    fn valid_exec(value: &str) -> bool {
        if value.is_empty() {
            return false;
        }

        let mut bytes = value.bytes().peekable();
        let mut quoted = false;
        let mut escaped = false;
        while let Some(byte) = bytes.next() {
            // The desktop entry string escapes backslashes before Exec quoting is read.
            if byte == b'\\' && bytes.peek() == Some(&b'\\') {
                bytes.next();
            }
            if escaped {
                escaped = false;
            } else if byte == b'"' {
                quoted = !quoted;
            } else if quoted && byte == b'\\' {
                escaped = true;
            }
        }
        !quoted
    }

    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut group = None;
    let mut found_entry = false;
    let mut entry_type = None;
    let mut has_exec = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let Some(name) = header.strip_suffix(']') else {
                return false;
            };
            if name.is_empty() || name.contains(['[', ']']) {
                return false;
            }
            if name == "Desktop Entry" {
                if found_entry {
                    return false;
                }
                found_entry = true;
            }
            group = Some(name);
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let key = key.trim();
        let value = value.trim();
        let name = if let Some((name, suffix)) = key.split_once('[') {
            let Some(locale) = suffix.strip_suffix(']') else {
                return false;
            };
            if locale.is_empty()
                || !locale
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.@-".contains(&byte))
            {
                return false;
            }
            name
        } else {
            key
        };
        if group.is_none()
            || name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || (key == "Exec" && !valid_exec(value))
        {
            return false;
        }
        if group == Some("Desktop Entry") {
            match (key, value) {
                ("Type", value) if entry_type.is_none() => entry_type = Some(value),
                ("Exec", _) if !has_exec => has_exec = true,
                ("Type" | "Exec", _)
                | ("Hidden", "true")
                | ("X-GNOME-Autostart-enabled", "false") => return false,
                _ => {}
            }
        }
    }
    entry_type == Some("Application") && has_exec
}

#[cfg(any(test, target_os = "windows"))]
fn windows_run_command(exe: &Path) -> Option<String> {
    // A Windows executable filename cannot contain quotes or end in a separator.
    let path = exe.to_str()?;
    (path.encode_utf16().count() + 2 <= 260).then(|| format!("\"{path}\""))
}

#[cfg(any(test, target_os = "windows"))]
fn windows_outcome(
    enabled: Option<bool>,
    run_present: bool,
    approval_disabled: bool,
) -> (bool, Result<AutostartStatus, AutostartError>) {
    let write_run = enabled == Some(true) && !run_present;
    let status = match enabled {
        Some(false) => Ok(AutostartStatus::Disabled),
        Some(true) if approval_disabled => Err(AutostartError::NeedsApproval),
        Some(true) => Ok(AutostartStatus::Enabled),
        None if !run_present => Ok(AutostartStatus::Disabled),
        None if approval_disabled => Ok(AutostartStatus::NeedsApproval),
        None => Ok(AutostartStatus::Enabled),
    };
    (write_run, status)
}

#[cfg(any(test, target_os = "windows"))]
fn startup_approved_disabled(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|byte| byte & 1 != 0)
}

#[cfg(any(test, target_os = "macos"))]
fn macos_status(raw: isize) -> Option<AutostartStatus> {
    match raw {
        0 | 3 => Some(AutostartStatus::Disabled),
        1 => Some(AutostartStatus::Enabled),
        2 => Some(AutostartStatus::NeedsApproval),
        _ => None,
    }
}

#[cfg(any(test, target_os = "macos"))]
fn in_app_bundle(exe: &Path) -> bool {
    let mut components = exe.components().rev();
    matches!(
        (components.next(), components.next(), components.next(), components.next()),
        (
            Some(std::path::Component::Normal(_)),
            Some(std::path::Component::Normal(macos)),
            Some(std::path::Component::Normal(contents)),
            Some(std::path::Component::Normal(bundle)),
        ) if macos == "MacOS" && contents == "Contents" && Path::new(bundle).extension().is_some_and(|ext| ext == "app")
    )
}

#[cfg(test)]
mod tests;
