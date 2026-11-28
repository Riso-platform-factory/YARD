use super::{
    AutostartError, AutostartStatus, Operation, OperationError, SystemBackend,
    startup_approved_disabled, windows_outcome, windows_run_command,
};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_ANY,
            RRF_RT_REG_BINARY, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
            RegOpenKeyExW, RegSetValueExW,
        },
    },
    core::{PCWSTR, w},
};

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const STARTUP_APPROVED: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");

impl SystemBackend {
    pub(super) fn operate(&self, enabled: Option<bool>) -> Operation {
        if !yard_product::is_identifier(&self.identifier) {
            return Err(AutostartError::Failed.into());
        }
        let identifier: Vec<u16> = self.identifier.encode_utf16().chain([0]).collect();
        let name = PCWSTR(identifier.as_ptr());

        if enabled == Some(false) {
            if let Some(run) = RunKey::open_for_write()? {
                // SAFETY: The handle is open for writes and name is a terminated UTF-16 string.
                present(unsafe { RegDeleteValueW(run.0, name) })?;
            }
            return Ok(AutostartStatus::Disabled);
        }

        // SAFETY: The key path and value name are terminated UTF-16 strings.
        let run_present = present(unsafe {
            RegGetValueW(HKEY_CURRENT_USER, RUN, name, RRF_RT_ANY, None, None, None)
        })?;
        let approval_disabled = (run_present || enabled == Some(true)) && approval_disabled(name)?;
        let (write_run, outcome) = windows_outcome(enabled, run_present, approval_disabled);
        if write_run {
            let exe = std::env::current_exe().map_err(OperationError::io)?;
            let command = windows_run_command(&exe).ok_or(AutostartError::Failed)?;
            let command: Vec<u8> = command
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            let run = RunKey::create()?;
            // SAFETY: The handle is open for writes. The name and REG_SZ data are terminated UTF-16.
            let result = unsafe { RegSetValueExW(run.0, name, None, REG_SZ, Some(&command)) };
            if result != ERROR_SUCCESS {
                return Err(OperationError::system(result.0));
            }
        }
        outcome.map_err(Into::into)
    }
}

fn approval_disabled(name: PCWSTR) -> Result<bool, OperationError> {
    let mut length = 0;
    // SAFETY: Both names are terminated UTF-16 strings. A null data pointer queries the size.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            STARTUP_APPROVED,
            name,
            RRF_RT_REG_BINARY,
            None,
            None,
            Some(&mut length),
        )
    };
    if !present(result)? || length == 0 {
        return Ok(false);
    }
    let mut bytes = vec![0; length as usize];
    // SAFETY: The buffer is valid for length bytes and the call updates length on return.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            STARTUP_APPROVED,
            name,
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut length),
        )
    };
    Ok(present(result)? && startup_approved_disabled(&bytes[..length as usize]))
}

struct RunKey(HKEY);

impl RunKey {
    fn open_for_write() -> Result<Option<Self>, OperationError> {
        let mut key = HKEY::default();
        // SAFETY: RUN is a terminated UTF-16 string and key is writable for the returned handle.
        let result =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, KEY_SET_VALUE, &mut key) };
        if present(result)? {
            Ok(Some(Self(key)))
        } else {
            Ok(None)
        }
    }

    fn create() -> Result<Self, OperationError> {
        let mut key = HKEY::default();
        // SAFETY: RUN is a terminated UTF-16 string and key is writable for the returned handle.
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                RUN,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        if result == ERROR_SUCCESS {
            Ok(Self(key))
        } else {
            Err(OperationError::system(result.0))
        }
    }
}

impl Drop for RunKey {
    fn drop(&mut self) {
        // SAFETY: Each RunKey owns one live handle returned by a successful open or create call.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn present(result: WIN32_ERROR) -> Result<bool, OperationError> {
    match result {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        _ => Err(OperationError::system(result.0)),
    }
}
