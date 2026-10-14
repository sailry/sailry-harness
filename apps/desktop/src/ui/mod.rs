//! Selected Bezel visuals on Kit's GPUI and the application's existing theme.
pub(crate) mod card_list;
pub(crate) mod details;
pub(crate) mod file_icon;
pub(crate) mod identicon;
pub(crate) mod keys;
pub(crate) mod loading;
mod motion;
#[cfg(any(test, feature = "workload-tests"))]
pub(crate) mod observation;
pub(crate) mod overlays;
pub(crate) mod pending;
pub(crate) mod session_preview;
pub(crate) mod settings_page;
pub(crate) mod status_list;
pub(crate) mod status_ring;
pub(crate) mod thinking;
pub(crate) mod webview_capture;
