//! User-requested input state snapshots contain no text or keystroke values.
use crate::shell::Shell;
use gpui_kit::{
    component::{WindowExt as _, input::AnyInputState},
    *,
};
use sailry_node_runtime::logging::{Area, Entry, Phase};
use serde::Serialize;

actions!(sailry, [CopyInputDiagnostics]);

#[derive(Serialize)]
struct Snapshot {
    version: u8,
    platform: &'static str,
    window_active: bool,
    composer_focused: Option<bool>,
    input_kind: &'static str,
    pending_prefix_count: usize,
    native: Native,
}

#[derive(Serialize)]
struct Native {
    available: bool,
    key_window: Option<bool>,
    responder: &'static str,
}

pub(crate) fn copy(
    shell: &mut Shell,
    _: &CopyInputDiagnostics,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    let composer_focused = shell
        .current_chat()
        .map(|chat| chat.read(cx).composer_focused(window, cx));
    let input_kind = match window.focused_input(cx) {
        Some(AnyInputState::Input(_)) => "input",
        Some(AnyInputState::Textarea(_)) => "textarea",
        Some(AnyInputState::Editor(_)) => "editor",
        Some(AnyInputState::Otp(_)) => "otp",
        None => "none",
    };
    let snapshot = Snapshot {
        version: 1,
        platform: std::env::consts::OS,
        window_active: window.is_window_active(),
        composer_focused,
        input_kind,
        pending_prefix_count: window
            .pending_input()
            .map_or(0, |pending| pending.keystrokes().len()),
        native: native(window),
    };
    record(&snapshot);
    // Only fixed categories, booleans and a count are serialized. No native
    // descriptions, pointers, input values or key sequences are inspected.
    cx.write_to_clipboard(ClipboardItem::new_string(
        serde_json::to_string_pretty(&snapshot).expect("input snapshot is serializable"),
    ));
    crate::feedback::info("", &crate::tr("input_diagnostics_copied"), window, cx);
}

fn record(snapshot: &Snapshot) {
    let flag = |value: Option<bool>| match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    for (stage, state) in [
        ("window_active", flag(Some(snapshot.window_active))),
        ("composer_focused", flag(snapshot.composer_focused)),
        ("input_kind", snapshot.input_kind),
        ("native_available", flag(Some(snapshot.native.available))),
        ("native_key_window", flag(snapshot.native.key_window)),
        ("native_responder", snapshot.native.responder),
    ] {
        let mut entry = Entry::new(Area::Desktop, Phase::Completed);
        entry.operation = Some("input_diagnostics");
        entry.stage = Some(stage);
        entry.source_kind = Some(state);
        entry.emit();
    }
    let mut entry = Entry::new(Area::Desktop, Phase::Completed);
    entry.operation = Some("input_diagnostics");
    entry.stage = Some("pending_prefix");
    entry.count = Some(snapshot.pending_prefix_count as u64);
    entry.emit();
}

#[cfg(not(target_os = "macos"))]
fn native(_: &Window) -> Native {
    Native {
        available: false,
        key_window: None,
        responder: "unavailable",
    }
}

#[cfg(target_os = "macos")]
fn native(window: &Window) -> Native {
    use objc2_app_kit::NSView;
    use objc2_web_kit::WKWebView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let unavailable = || Native {
        available: false,
        key_window: None,
        responder: "unknown",
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return unavailable();
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return unavailable();
    };
    // SAFETY: GPUI owns this live NSView; the Help action runs on its AppKit
    // thread. Native objects stay borrowed here and no address is recorded.
    let parent = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let Some(native) = parent.window() else {
        return unavailable();
    };
    let responder = native.firstResponder();
    let kind = if let Some(responder) = responder.as_deref() {
        if let Some(view) = responder.downcast_ref::<NSView>() {
            let mut kind = if std::ptr::eq(view, parent) {
                "gpui"
            } else {
                "other"
            };
            if view.downcast_ref::<WKWebView>().is_some() {
                kind = "webview";
            }
            let mut ancestor = view.superview();
            for _ in 0..64 {
                let Some(view) = ancestor else {
                    break;
                };
                if view.downcast_ref::<WKWebView>().is_some() {
                    kind = "webview";
                    break;
                }
                if std::ptr::eq(&*view, parent) {
                    if kind == "other" {
                        kind = "gpui";
                    }
                    break;
                }
                ancestor = view.superview();
            }
            kind
        } else {
            "other"
        }
    } else {
        "none"
    };
    Native {
        available: true,
        key_window: Some(native.isKeyWindow()),
        responder: kind,
    }
}
