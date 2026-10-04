use super::*;
use sailry_protocol::conversation::Entry;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;

pub(super) fn button(entry: &Entry, cx: &mut Context<View>) -> AnyElement {
    let entry = Arc::new(entry.clone());
    crate::conversation::disclosure::trigger(
        format!("search-{}", entry.id),
        IconName::Globe,
        tr("chat_search_result"),
        crate::conversation::disclosure::Detail::default(),
        false,
        cx,
    )
    .debug_selector(|| "live-search-result".into())
    .on_click(cx.listener(move |view, _, _, cx| {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if !native::open(entry.clone(), cx) {
            view.error = Some("chat_search_unavailable");
            cx.notify();
        }
    }))
    .disabled(!cfg!(any(target_os = "macos", target_os = "windows")))
    .into_any_element()
}

pub(super) fn answer(entry: &Entry) -> String {
    entry
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>()
}

fn destination(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
    })
}
