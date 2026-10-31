use crate::Labels;
use gpui_kit::App;

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Labels {
        note: "Note".into(),
        tip: "Tip".into(),
        important: "Important".into(),
        warning: "Warning".into(),
        caution: "Caution".into(),
        plain: "Plain text".into(),
        copy_code: "Copy".into(),
        copied_code: "Copied".into(),
        caption: "Caption".into(),
        add_image: "Add image".into(),
        undo: "Undo".into(),
        redo: "Redo".into(),
        copy: "Copy".into(),
        cut: "Cut".into(),
        paste: "Paste".into(),
        link_address: "Link address".into(),
        link_open: "Open link".into(),
        link_edit: "Edit link".into(),
        link_copy: "Copy link".into(),
        link_remove: "Remove link".into(),
        image_address: "Image address".into(),
        image_alt: "Alt text".into(),
        image_choose: "Choose file".into(),
    });
}
