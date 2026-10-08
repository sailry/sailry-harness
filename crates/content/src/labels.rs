use gpui_kit::{Global, SharedString};

/// Document control labels supplied by the application's existing localization.
pub struct Labels {
    pub note: SharedString,
    pub tip: SharedString,
    pub important: SharedString,
    pub warning: SharedString,
    pub caution: SharedString,
    pub plain: SharedString,
    pub copy_code: SharedString,
    pub copied_code: SharedString,
    pub caption: SharedString,
    pub add_image: SharedString,
    pub undo: SharedString,
    pub redo: SharedString,
    pub copy: SharedString,
    pub cut: SharedString,
    pub paste: SharedString,
}

impl Global for Labels {}
