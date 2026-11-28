#[cfg(target_os = "linux")]
use super::SystemBackend;
use super::{
    AutostartError, AutostartStatus, Operation, OperationError, desktop_entry,
    desktop_entry_enabled,
};
#[cfg(target_os = "linux")]
use std::{env, path::PathBuf};
use std::{fs, io::ErrorKind, path::Path};

#[cfg(target_os = "linux")]
impl SystemBackend {
    pub(super) fn operate(&self, enabled: Option<bool>) -> Operation {
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| {
                env::home_dir()
                    .filter(|home| home.is_absolute())
                    .map(|home| home.join(".config"))
            })
            .ok_or(AutostartError::Failed)?;
        linux_autostart_with_exe(
            &config.join("autostart"),
            &self.identifier,
            &self.product_name,
            enabled,
            || env::current_exe().map_err(OperationError::io),
        )
    }
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "test adapter for the shared Linux operation")
)]
pub(super) fn linux_autostart(
    dir: &Path,
    identifier: &str,
    product_name: &str,
    exe: &Path,
    enabled: Option<bool>,
) -> Result<AutostartStatus, AutostartError> {
    linux_autostart_with_exe(dir, identifier, product_name, enabled, || Ok(exe))
        .map_err(|error| error.error)
}

fn linux_autostart_with_exe<P: AsRef<Path>>(
    dir: &Path,
    identifier: &str,
    product_name: &str,
    enabled: Option<bool>,
    exe: impl FnOnce() -> Result<P, OperationError>,
) -> Operation {
    if !yard_product::is_identifier(identifier) {
        return Err(AutostartError::Failed.into());
    }
    let path = dir.join(format!("{identifier}.desktop"));
    let current = fs::read_to_string(&path).is_ok_and(|entry| desktop_entry_enabled(&entry));

    match enabled {
        Some(true) if !current => {
            let exe = exe()?;
            let exe = exe.as_ref();
            if !exe.to_str().is_some_and(|path| !path.contains('=')) {
                return Err(AutostartError::Failed.into());
            }
            let entry = desktop_entry(product_name, exe);
            yard_paths::write_atomic(&path, entry.as_bytes()).map_err(OperationError::io)?;
            Ok(AutostartStatus::Enabled)
        }
        Some(false) if current => {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(OperationError::io(error)),
            }
            Ok(AutostartStatus::Disabled)
        }
        _ if current => Ok(AutostartStatus::Enabled),
        _ => Ok(AutostartStatus::Disabled),
    }
}
