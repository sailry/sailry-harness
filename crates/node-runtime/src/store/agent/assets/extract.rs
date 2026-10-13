use super::*;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(super) fn items(entry: &Entry, root: Option<&str>) -> Vec<Target> {
    let mut items = Vec::new();
    for part in &entry.parts {
        match part {
            Part::Attachment(value) => push(&mut items, Target::Attachment(value.clone())),
            Part::Image(value) => push(&mut items, Target::Image(value.clone())),
            Part::ToolResult { result, images, .. } => {
                if let Some(content) = sailry_protocol::tool::Presentation::Content.content(result)
                {
                    for block in content.blocks {
                        match block {
                            sailry_protocol::tool::Block::File(file) => {
                                push(&mut items, Target::File { path: file.path });
                            }
                            sailry_protocol::tool::Block::Artifact { artifact: document } => {
                                push(&mut items, Target::Document(document));
                            }
                            _ => {}
                        }
                    }
                }
                for image in images {
                    push(&mut items, Target::Image(image.clone()));
                }
            }
            Part::Reference(reference) => {
                if let reference::Target::File(path) = &reference.target {
                    push(&mut items, Target::File { path: path.clone() });
                }
            }
            Part::Text(text) => {
                markdown(&mut items, text, root, entry.author == "user");
            }
            _ => {}
        }
    }
    items
}

fn markdown(items: &mut Vec<Target>, text: &str, root: Option<&str>, input: bool) {
    let mut paragraph = false;
    let mut link = false;
    let mut standalone = false;
    let mut delivery = None;
    let mut links = 0;
    let mut quotes = 0;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::BlockQuote(_)) => quotes += 1,
            Event::End(TagEnd::BlockQuote(_)) => quotes -= 1,
            Event::Start(Tag::Paragraph | Tag::Item) => {
                paragraph = true;
                standalone = quotes == 0;
                delivery = None;
                links = 0;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = true;
                if input {
                    if let Some(path) = path_target(&dest_url, root) {
                        push(items, Target::File { path });
                    }
                } else if paragraph {
                    links += 1;
                    if links > 1 {
                        standalone = false;
                    }
                    // A section or source-line link is navigation, not a delivery.
                    let line = dest_url.rsplit_once(':').is_some_and(|(_, value)| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    });
                    delivery = (!dest_url.contains(['#', '?']) && !line)
                        .then(|| path_target(&dest_url, root))
                        .flatten()
                        .map(|path| Target::File { path });
                }
            }
            Event::End(TagEnd::Link) => link = false,
            Event::Start(Tag::Image { dest_url, .. }) => {
                standalone = false;
                if let Some(path) = path_target(&dest_url, root) {
                    push(items, Target::File { path });
                }
            }
            Event::Text(value) | Event::Code(value) if !link && !value.trim().is_empty() => {
                standalone = false;
            }
            Event::Html(_) | Event::InlineHtml(_) => standalone = false,
            Event::End(TagEnd::Paragraph | TagEnd::Item) => {
                if standalone && let Some(target) = delivery.take() {
                    push(items, target);
                }
                paragraph = false;
            }
            _ => {}
        }
    }
}

fn push(items: &mut Vec<Target>, item: Target) {
    if !items.contains(&item) {
        items.push(item);
    }
}

fn path_target(link: &str, root: Option<&str>) -> Option<String> {
    if link.starts_with('#') || link.is_empty() {
        return None;
    }
    if root.is_none() && (link.starts_with('/') || link.contains([':', '\\'])) {
        return None;
    }
    let root = root.unwrap_or("/").replace('\\', "/");
    let mut base = url::Url::parse("file:///").ok()?;
    base.set_path(&format!("{}/", root.trim_end_matches('/')));
    let root = base.to_file_path().ok()?;
    let link = link.split(['#', '?']).next()?;
    let link = link
        .rsplit_once(':')
        .filter(|(_, line)| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
        .map_or(link, |(path, _)| path);
    let link = link.replace('\\', "/");
    let resolved = if link.as_bytes().get(1) == Some(&b':') {
        let mut url = url::Url::parse("file:///").ok()?;
        url.set_path(&link);
        url
    } else {
        base.join(&link).ok()?
    };
    if resolved.scheme() != "file" {
        return None;
    }
    let path = resolved.to_file_path().ok()?;
    let path = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    Some(path)
}
