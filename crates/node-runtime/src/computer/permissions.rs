//! Permission checks never prompt; only an explicit local request opens system settings.
use sailry_protocol::{
    ErrorCode, Fault, NodeId,
    computer::{Permission, Permissions},
};

pub(crate) fn read(node: NodeId, caller: NodeId) -> Permissions {
    let state = status();
    Permissions {
        node,
        platform: std::env::consts::OS.into(),
        local: node == caller,
        screen_capture: state["screen_capture"].as_bool(),
        accessibility: state["input"].as_bool(),
    }
}

pub(crate) fn request(
    node: NodeId,
    caller: NodeId,
    permission: Permission,
) -> Result<Permissions, Fault> {
    if node != caller {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "System permissions must be granted on the execution device",
        ));
    }
    open(permission)?;
    Ok(read(node, caller))
}

#[cfg(target_os = "macos")]
fn open(permission: Permission) -> Result<(), Fault> {
    use objc2_app_kit::NSWorkspace;
    use objc2_application_services::{
        AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
    };
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFType};
    use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
    use objc2_foundation::{NSString, NSURL};

    let page = match permission {
        Permission::ScreenCapture => {
            if !CGPreflightScreenCaptureAccess() {
                CGRequestScreenCaptureAccess();
            }
            "Privacy_ScreenCapture"
        }
        Permission::Accessibility => {
            // Apple's documented option contains one CFString key and CFBoolean value.
            unsafe {
                if !AXIsProcessTrusted() {
                    let options = CFDictionary::<CFType, CFType>::from_slices(
                        &[kAXTrustedCheckOptionPrompt.as_ref()],
                        &[CFBoolean::new(true).as_ref()],
                    );
                    AXIsProcessTrustedWithOptions(Some(options.as_opaque()));
                }
            }
            "Privacy_Accessibility"
        }
    };
    let url = NSURL::URLWithString(&NSString::from_str(&format!(
        "x-apple.systempreferences:com.apple.preference.security?{page}"
    )))
    .ok_or_else(|| Fault::new(ErrorCode::Internal, "Invalid system settings URL"))?;
    if !NSWorkspace::sharedWorkspace().openURL(&url) {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "Could not open system settings",
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open(_: Permission) -> Result<(), Fault> {
    Err(Fault::new(
        ErrorCode::Unavailable,
        "System permission guidance is not supported on this platform",
    ))
}

pub(super) fn status() -> serde_json::Value {
    #[cfg(target_os = "macos")]
    {
        let status = cua_driver_sdk::current_mac_os_permission_status();
        serde_json::json!({"screen_capture":status.screen_recording,"input":status.accessibility})
    }
    #[cfg(not(target_os = "macos"))]
    {
        serde_json::json!({"screen_capture":null,"input":null})
    }
}
