use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
use sailry_protocol::{ErrorCode, Fault};
use std::sync::{Arc, Mutex};

/// Elements that never carry readable page content.
const SKIPPED: [&str; 17] = [
    "script", "style", "template", "nav", "footer", "aside", "noscript", "iframe", "svg", "canvas",
    "object", "embed", "form", "button", "select", "textarea", "dialog",
];
/// Containers whose ARIA role or visibility can mark page chrome.
const CONTAINERS: [&str; 8] = ["div", "section", "header", "ul", "ol", "span", "table", "p"];

#[derive(Default)]
struct Captured {
    title: Option<String>,
    main: Option<String>,
    articles: Vec<String>,
}

/// Converts HTML to Markdown, preferring the `main` landmark or a single
/// `article` over the whole body so page chrome does not crowd the content.
pub(super) fn convert(html: &str) -> Result<(Option<String>, String), Fault> {
    let captured = Arc::new(Mutex::new(Captured::default()));
    let (title, main, landmarks, articles) = (
        captured.clone(),
        captured.clone(),
        captured.clone(),
        captured.clone(),
    );
    let converter = HtmlToMarkdown::builder()
        .skip_tags(SKIPPED.to_vec())
        .add_handler(vec!["head"], |handlers: &dyn Handlers, element: Element| {
            handlers.walk_children(element.node);
            None
        })
        .add_handler(
            vec!["title"],
            move |handlers: &dyn Handlers, element: Element| {
                let text = handlers.walk_children(element.node).content;
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let mut captured = title.lock().expect("web capture lock is healthy");
                if !text.is_empty() && captured.title.is_none() {
                    captured.title = Some(text);
                }
                None
            },
        )
        .add_handler(
            CONTAINERS.to_vec(),
            move |handlers: &dyn Handlers, element: Element| {
                let attribute = |name: &str| {
                    element
                        .attrs
                        .iter()
                        .find(|attribute| &*attribute.name.local == name)
                        .map(|attribute| attribute.value.trim().to_ascii_lowercase())
                };
                let role = attribute("role");
                if attribute("hidden").is_some()
                    || attribute("aria-hidden").as_deref() == Some("true")
                    || matches!(
                        role.as_deref(),
                        Some("navigation" | "banner" | "contentinfo" | "complementary" | "search")
                    )
                {
                    return None;
                }
                let result = handlers.fallback(element);
                if role.as_deref() == Some("main") {
                    keep(&landmarks, result.as_ref());
                }
                result
            },
        )
        .add_handler(
            vec!["main"],
            move |handlers: &dyn Handlers, element: Element| {
                let result = handlers.fallback(element);
                keep(&main, result.as_ref());
                result
            },
        )
        .add_handler(
            vec!["article"],
            move |handlers: &dyn Handlers, element: Element| {
                let result = handlers.fallback(element);
                if let Some(result) = &result {
                    articles
                        .lock()
                        .expect("web capture lock is healthy")
                        .articles
                        .push(result.content.clone());
                }
                result
            },
        )
        .build();
    let markdown = converter
        .convert(html)
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "web HTML extraction failed"))?;
    let mut captured = captured.lock().expect("web capture lock is healthy");
    let title = captured.title.take();
    let primary = captured.main.take().or_else(|| {
        // Several articles usually form a listing; keep the whole page then.
        (captured.articles.len() == 1).then(|| captured.articles.remove(0))
    });
    let content = primary
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or(markdown);
    Ok((title, content))
}

fn keep(captured: &Mutex<Captured>, result: Option<&htmd::element_handler::HandlerResult>) {
    let mut captured = captured.lock().expect("web capture lock is healthy");
    if captured.main.is_none()
        && let Some(result) = result.filter(|result| !result.content.trim().is_empty())
    {
        captured.main = Some(result.content.clone());
    }
}

#[cfg(test)]
mod tests;
