//! "Launch at Login" via `SMAppService.mainAppService`: macOS lists the bundle itself in
//! System Settings → General → Login Items. Works only when running from `Barkley.app`.

use objc2_service_management::{SMAppService, SMAppServiceStatus};

fn status() -> SMAppServiceStatus {
    unsafe { SMAppService::mainAppService().status() }
}

pub fn is_enabled() -> bool {
    status() == SMAppServiceStatus::Enabled
}

/// Registers when off, unregisters when on (or still awaiting approval).
pub fn toggle() -> Result<(), String> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = match unsafe { service.status() } {
        SMAppServiceStatus::Enabled | SMAppServiceStatus::RequiresApproval => unsafe {
            service.unregisterAndReturnError()
        },
        _ => unsafe { service.registerAndReturnError() },
    };
    result.map_err(|e| e.localizedDescription().to_string())?;
    // The user turned Barkley off in Login Items earlier; only they can turn it back on there.
    if unsafe { service.status() } == SMAppServiceStatus::RequiresApproval {
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
    Ok(())
}
