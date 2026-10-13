//! Shared file artwork for conversation resources and attachments.
use gpui_kit::*;

mod resolution;
#[cfg(test)]
mod tests;

pub(crate) fn render(path: &str, mime: &str, size: Rems) -> AnyElement {
    let kind = resolution::resolve(path, mime);
    let edition = if size.0 <= 1.3125 {
        "compact"
    } else {
        "regular"
    };
    let asset = format!("icons/reui-file-type/{edition}/{kind}.svg");
    // Kit Icon applies one foreground color; the supported image path preserves
    // ReUI's filled page, folded corner and contrasting extension label.
    img(SharedString::from(asset))
        .size(size)
        .flex_shrink_0()
        .debug_selector(move || format!("file-type-{kind}"))
        .into_any_element()
}
