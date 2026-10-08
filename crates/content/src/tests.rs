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
    });
}
