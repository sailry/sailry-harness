//! Shared document presentation for conversations and resource views.
//!
//! Components consume content and emit editing or navigation events. Resource
//! ownership, requests and persistence stay with their existing feature modules.

pub(crate) mod images;
#[cfg(test)]
pub(crate) use sailry_content::syntax;
pub(crate) use sailry_content::{diff, editor, markdown, selection};

pub(crate) mod files;

pub(crate) fn init(cx: &mut gpui_kit::App) {
    use crate::tr;
    cx.set_global(sailry_content::Labels {
        note: tr("content_note"),
        tip: tr("content_tip"),
        important: tr("content_important"),
        warning: tr("content_warning"),
        caution: tr("content_caution"),
        plain: tr("content_plain"),
        copy_code: tr("content_copy"),
        copied_code: tr("content_copied"),
        caption: tr("content_caption"),
        add_image: tr("content_add_image"),
        undo: tr("files_undo"),
        redo: tr("files_redo"),
        copy: tr("files_copy"),
        cut: tr("files_cut"),
        paste: tr("files_paste"),
    });
}
