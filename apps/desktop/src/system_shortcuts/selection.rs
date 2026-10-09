//! Read the foreground selection without changing its text or clipboard.
#[cfg(target_os = "macos")]
pub(super) fn read() -> Result<String, &'static str> {
    use core_foundation::base::{CFRelease, CFTypeRef};
    use cua_platform_macos::ax::bindings::*;
    use objc2_app_kit::NSWorkspace;
    // SAFETY: these AnyThread AppKit queries and owned AX copies are valid on
    // the worker. Each copied element is released once after its last use.
    unsafe {
        if !AXIsProcessTrusted() {
            return Err("selection_permission");
        }
        let application = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .ok_or("selection_empty")?;
        let element = AXUIElementCreateApplication(application.processIdentifier());
        if element.is_null() {
            return Err("selection_unavailable");
        }
        AXUIElementSetMessagingTimeout(element, 0.5);
        let focused = copy_element_attr(element, "AXFocusedUIElement");
        CFRelease(element as CFTypeRef);
        let Some(focused) = focused else {
            return Err("selection_empty");
        };
        let protected =
            copy_string_attr(focused, "AXSubrole").is_some_and(|role| role == "AXSecureTextField");
        let text = if protected {
            None
        } else {
            copy_string_attr(focused, "AXSelectedText")
        };
        CFRelease(focused as CFTypeRef);
        bounded(text.ok_or("selection_empty")?)
    }
}

#[cfg(target_os = "windows")]
pub(super) fn read() -> Result<String, &'static str> {
    use windows::Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
        },
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    // SAFETY: successful MTA initialization is paired with CoUninitialize.
    // All interfaces belong to this worker and are released before the apartment.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .map_err(|_| "selection_unavailable")?;
        let _apartment = Apartment;
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| "selection_unavailable")?;
        let focused = automation
            .GetFocusedElement()
            .map_err(|_| "selection_empty")?;
        if focused
            .CurrentIsPassword()
            .map_err(|_| "selection_unavailable")?
            .as_bool()
        {
            return Err("selection_empty");
        }
        let pattern: IUIAutomationTextPattern = focused
            .GetCurrentPatternAs(UIA_TextPatternId)
            .map_err(|_| "selection_empty")?;
        let ranges = pattern.GetSelection().map_err(|_| "selection_empty")?;
        if ranges.Length().map_err(|_| "selection_unavailable")? != 1 {
            return Err("selection_empty");
        }
        let text = ranges
            .GetElement(0)
            .and_then(|range| range.GetText(65537))
            .map_err(|_| "selection_unavailable")?
            .to_string();
        bounded(text)
    }
}

#[cfg(target_os = "linux")]
pub(super) fn read() -> Result<String, &'static str> {
    use std::time::{Duration, Instant};
    use x11rb::{
        connection::Connection,
        protocol::{
            Event,
            xproto::{AtomEnum, ConnectionExt, CreateWindowAux, EventMask, WindowClass},
        },
    };
    let (connection, screen) = x11rb::connect(None).map_err(|_| "selection_unavailable")?;
    let window = connection
        .generate_id()
        .map_err(|_| "selection_unavailable")?;
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            connection.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|_| "selection_unavailable")?;
    let utf8 = connection
        .intern_atom(false, b"UTF8_STRING")
        .map_err(|_| "selection_unavailable")?
        .reply()
        .map_err(|_| "selection_unavailable")?
        .atom;
    let property = connection
        .intern_atom(false, b"SAILRY_SELECTION")
        .map_err(|_| "selection_unavailable")?
        .reply()
        .map_err(|_| "selection_unavailable")?
        .atom;
    connection
        .convert_selection(
            window,
            AtomEnum::PRIMARY,
            utf8,
            property,
            x11rb::CURRENT_TIME,
        )
        .map_err(|_| "selection_unavailable")?;
    connection.flush().map_err(|_| "selection_unavailable")?;
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        if let Some(Event::SelectionNotify(event)) = connection
            .poll_for_event()
            .map_err(|_| "selection_unavailable")?
        {
            if event.requestor == window && event.property == property {
                let response = connection
                    .get_property(true, window, property, utf8, 0, 16385)
                    .map_err(|_| "selection_unavailable")?
                    .reply()
                    .map_err(|_| "selection_unavailable")?;
                if response.bytes_after != 0 || response.type_ != utf8 {
                    return Err("selection_unavailable");
                }
                return bounded(
                    String::from_utf8(response.value).map_err(|_| "selection_unavailable")?,
                );
            }
            return Err("selection_empty");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err("selection_empty")
}

fn bounded(text: String) -> Result<String, &'static str> {
    if text.len() > 65536 || text.contains('\0') {
        return Err("selection_large");
    }
    Ok(text)
}
