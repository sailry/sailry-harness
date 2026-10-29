use super::*;

pub(super) fn render(
    view: &View,
    id: &str,
    call: &Call,
    data: &sailry_protocol::tool::Text,
    result: &serde_json::Value,
    grouped: bool,
    cx: &mut Context<View>,
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
            column.child(view.literal_text(id, call, value, cx))
        })
        .children(data.notices.iter().map(|notice| {
            surface::notice(notice.label(&rust_i18n::locale()).to_owned(), cx)
                .py_0()
                .pb_2()
        }));
    if grouped {
        body.into_any_element()
    } else {
        surface::scroll(format!("{id}-scroll"), body)
    }
}
