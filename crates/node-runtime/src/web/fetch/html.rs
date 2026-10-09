use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
use sailry_protocol::{ErrorCode, Fault};
use std::sync::{Arc, Mutex};

pub(super) fn convert(html: &str) -> Result<(Option<String>, String), Fault> {
    let title = Arc::new(Mutex::new(None));
    let captured = title.clone();
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "template", "nav", "footer"])
        .add_handler(vec!["head"], |handlers: &dyn Handlers, element: Element| {
            handlers.walk_children(element.node);
            None
        })
        .add_handler(
            vec!["title"],
            move |handlers: &dyn Handlers, element: Element| {
                let text = handlers.walk_children(element.node).content;
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    let mut title = captured.lock().expect("web title lock is healthy");
                    if title.is_none() {
                        *title = Some(text);
                    }
                }
                None
            },
        )
        .build();
    let markdown = converter
        .convert(html)
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "web HTML extraction failed"))?;
    let title = title.lock().expect("web title lock is healthy").take();
    Ok((title, markdown))
}

#[cfg(test)]
mod tests;
