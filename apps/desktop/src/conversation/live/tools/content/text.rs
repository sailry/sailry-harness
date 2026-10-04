use super::*;

pub(super) fn render(
    id: &str,
    data: &sailry_protocol::tool::Text,
    result: &serde_json::Value,
    grouped: bool,
    cx: &App,
) -> AnyElement {
    let value = data.read(result).unwrap_or_default();
    let body = v_flex()
        .w_full()
        .min_w_0()
        .debug_selector({
            let id = id.to_owned();
            move || id.clone()
        })
        .when(!value.is_empty(), |column| {
            column.child(surface::literal_text(id, value, cx))
        })
        .children(data.notices.iter().map(|notice| {
            surface::notice(notice.label(&rust_i18n::locale()).to_owned(), false, cx)
                .py_0()
                .pb_2()
        }));
    if grouped {
        body.into_any_element()
    } else {
        surface::scroll(format!("{id}-scroll"), body)
    }
}

pub(super) fn copy(content: &Content, result: &serde_json::Value, locale: &str) -> String {
    let text = content
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) => text.read(result).filter(|text| !text.is_empty()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        return text;
    }
    content
        .blocks
        .iter()
        .flat_map(|block| match block {
            Block::Text(text) => text
                .notices
                .iter()
                .map(|notice| notice.label(locale))
                .collect(),
            Block::Notice { message } => vec![message.label(locale)],
            _ => Vec::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
