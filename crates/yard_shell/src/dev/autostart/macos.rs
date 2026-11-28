use super::{
    AutostartError, AutostartStatus, Operation, OperationError, SystemBackend, in_app_bundle,
    macos_status,
};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSError, NSOperatingSystemVersion, NSProcessInfo};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

// From the SDK's SMErrors.h.
const JOB_NOT_FOUND: isize = 6;
const LAUNCH_DENIED_BY_USER: isize = 11;
const ALREADY_REGISTERED: isize = 12;

impl SystemBackend {
    pub(super) fn operate(&self, enabled: Option<bool>) -> Operation {
        let exe = std::env::current_exe().map_err(OperationError::io)?;
        let exe = exe.canonicalize().map_err(OperationError::io)?;
        if !in_app_bundle(&exe)
            || !NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(
                NSOperatingSystemVersion {
                    majorVersion: 13,
                    minorVersion: 0,
                    patchVersion: 0,
                },
            )
        {
            return unsupported(enabled);
        }
        let Some(class) = AnyClass::get(c"SMAppService") else {
            return unsupported(enabled);
        };
        // SAFETY: This is the class method from SMAppService.h, available on macOS 13+.
        let service: Option<Retained<AnyObject>> = unsafe { msg_send![class, mainAppService] };
        let Some(service) = service else {
            return unsupported(enabled);
        };
        let current = status(&service)?;
        let Some(enabled) = enabled else {
            return Ok(current);
        };
        match (current, enabled) {
            (AutostartStatus::NeedsApproval, true) => {
                return Err(AutostartError::NeedsApproval.into());
            }
            (AutostartStatus::Enabled, true) | (AutostartStatus::Disabled, false) => {
                return Ok(current);
            }
            _ => {}
        }

        let mut error: Option<Retained<NSError>> = None;
        // SAFETY: The documented selectors return BOOL and write an NSError to this out parameter.
        let success: bool = unsafe {
            if enabled {
                msg_send![&*service, registerAndReturnError: &mut error]
            } else {
                msg_send![&*service, unregisterAndReturnError: &mut error]
            }
        };
        if success {
            return status(&service);
        }
        let error = error.ok_or(AutostartError::Failed)?;
        let code = error.code();
        let domain = error.domain();
        if &*domain == objc2_foundation::ns_string!("SMAppServiceErrorDomain")
            || &*domain == objc2_foundation::ns_string!("SMJobErrorDomain")
        {
            match (enabled, code) {
                (true, ALREADY_REGISTERED) => return status(&service),
                (true, LAUNCH_DENIED_BY_USER) => return Err(AutostartError::NeedsApproval.into()),
                (false, JOB_NOT_FOUND) => return Ok(AutostartStatus::Disabled),
                _ => {}
            }
        }
        Err(OperationError::system(code as i64))
    }
}

fn status(service: &AnyObject) -> Operation {
    // SAFETY: The receiver is an SMAppService from mainAppService. Its status is an NSInteger.
    let raw: isize = unsafe { msg_send![service, status] };
    macos_status(raw).ok_or_else(|| OperationError::system(raw as i64))
}

fn unsupported(enabled: Option<bool>) -> Operation {
    match enabled {
        Some(_) => Err(AutostartError::NotSupported.into()),
        None => Ok(AutostartStatus::NotSupported),
    }
}
