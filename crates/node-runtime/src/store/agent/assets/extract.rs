use super::*;
use pulldown_cmark::{Event, Parser, Tag};

pub(super) fn items(entry: &Entry, root: Option<&str>) -> Vec<Target> {
    let mut items = Vec::new();
    for part in &entry.parts {
        match part {
            Part::Attachment(value) => push(&mut items, Target::Attachment(value.clone())),
            Part::Image(value) => push(&mut items, Target::Image(value.clone())),
            Part::ToolResult { result, images, .. } => {
                {
                    let file = match serde_json::from_value(result.clone()) {
                        Ok(Output::FileWritten(file)) => Some(file),
                        Ok(Output::OfficeWritten(written)) => Some(written.file),
                        _ => None,
                    };
                    if let Some(file) = file {
                        push(&mut items, Target::File { path: file.path });
                    }
                }
                if let Some(content) = sailry_protocol::tool::Presentation::Content.content(result)
                {
                    for block in content.blocks {
                        if let sailry_protocol::tool::Block::File(file) = block {
                            push(&mut items, Target::File { path: file.path });
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
                // Only explicit Markdown destinations count; prose and code are not file evidence.
                for event in Parser::new(text) {
                    if let Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) =
                        event
                        && let Some(path) = path_target(&dest_url, root)
                    {
                        push(&mut items, Target::File { path });
                    }
                }
            }
            _ => {}
        }
    }
    items
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
