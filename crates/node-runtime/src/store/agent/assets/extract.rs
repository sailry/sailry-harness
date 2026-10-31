use super::*;
use crate::agent::canonical::BlockKind;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(super) fn items(entry: &Entry, root: Option<&str>) -> Vec<Target> {
    let mut items = Vec::new();
    for part in &entry.parts {
        match part {
            Part::Attachment(value) => push(&mut items, Target::Attachment(value.clone())),
            Part::Image(value) => push(&mut items, Target::Image(value.clone())),
            Part::ToolResult { images, .. } => {
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

/// Projects only the parts `items` reads, in the same order as the history projection.
/// The full projection resolves every tool result's request with a per-turn scan, which
/// made asset pages quadratic in long turns; inventory needs only images and text.
pub(super) async fn entry(
    db: &Connection,
    sequence: u64,
    turn: TurnId,
    event: &Record,
) -> Result<Entry, Fault> {
    let references = super::super::references::project(event)?;
    let input = event.metadata.input.as_ref();
    let skipped = usize::from(!references.is_empty())
        + usize::from(input.is_some_and(|input| input.notice_part.is_some()));
    let hidden = event.provenance.author == "system"
        && event.context().is_some_and(|context| context.captured);
    let capability = input.and_then(|input| input.capability_part);
    let mut parts = Vec::new();
    if !hidden {
        if let Some(message) = event.message() {
            for index in skipped..message.part_count {
                if Some(index) == capability {
                    continue;
                }
                let Some(block) = message.content.iter().find(|block| block.index == index) else {
                    continue;
                };
                match &block.kind {
                    BlockKind::Text { text } => parts.push(Part::Text(text.clone())),
                    BlockKind::Image { asset } => parts.push(Part::Image(
                        super::super::images::inline(db, turn, event, index, asset).await?,
                    )),
                    BlockKind::File { file }
                        if crate::agent::attachments::is_reference(&file.uri) =>
                    {
                        let id = crate::agent::attachments::id(&file.uri)?;
                        let attachment =
                            crate::store::attachments::turns::get(db, turn, id).await?;
                        if file.mime.as_deref() != Some(attachment.spec.media_type.as_str()) {
                            return Err(Fault::invalid("attachment media type mismatch"));
                        }
                        parts.push(Part::Attachment(attachment));
                    }
                    BlockKind::ToolResponse(response) => parts.push(Part::ToolResult {
                        id: Some(response.id.clone()),
                        name: response.name.clone(),
                        summary: Default::default(),
                        images: super::super::images::project(db, turn, event, index, response)
                            .await?,
                    }),
                    _ => {}
                }
            }
        }
        parts.extend(references.into_iter().map(Part::Reference));
    }
    Ok(Entry {
        sequence,
        turn,
        id: event.id.clone(),
        author: event.provenance.author.clone(),
        branch: event.provenance.branch.clone(),
        timestamp_ms: event.created.timestamp_millis(),
        parts,
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    })
}

pub(super) async fn publications(
    db: &Connection,
    items: &mut Vec<Target>,
    event: &Record,
) -> Result<(), Fault> {
    for block in event
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
    {
        let BlockKind::ToolResponse(response) = &block.kind else {
            continue;
        };
        let result = super::super::blobs::hydrate(db, &response.result).await?;
        let value = super::super::history::tools::value(response.format, &result);
        let Some(content) = sailry_protocol::tool::Presentation::Content.content(value) else {
            continue;
        };
        for block in content.blocks {
            match block {
                sailry_protocol::tool::Block::File(file) => {
                    push(items, Target::File { path: file.path })
                }
                sailry_protocol::tool::Block::Artifact { artifact } => {
                    push(items, Target::Document(artifact))
                }
                _ => {}
            }
        }
    }
    Ok(())
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
