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
    let root_path = node_path(root.unwrap_or("/"));
    let mut base = url::Url::parse("file:///").ok()?;
    base.set_path(&format!(
        "{}/",
        root_path.trim_end_matches('/').replace('%', "%25")
    ));
    let link = node_path(link);
    let link = link.split(['#', '?']).next()?;
    let link = link
        .rsplit_once(':')
        .filter(|(_, line)| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
        .map_or(link, |(path, _)| path);
    if root.is_none() {
        let decoded = percent_encoding::percent_decode_str(link)
            .decode_utf8()
            .ok()?;
        crate::files::path::components_with(
            decoded.trim_start_matches("./"),
            false,
            crate::files::Access::Manual,
        )
        .ok()?;
    }
    let mut resolved = if link.as_bytes().get(1) == Some(&b':') || link.starts_with("//") {
        let mut url = url::Url::parse("file:///").ok()?;
        url.set_path(link);
        url
    } else {
        base.join(link).ok()?
    };
    for url in [&mut base, &mut resolved] {
        let path = url.path();
        if path.as_bytes().get(1).is_some_and(u8::is_ascii_lowercase)
            && path.as_bytes().get(2) == Some(&b':')
            && (path.len() == 3 || path.as_bytes().get(3) == Some(&b'/'))
        {
            let mut path = path.to_owned();
            path[1..2].make_ascii_uppercase();
            url.set_path(&path);
        }
    }
    let relative = base.make_relative(&resolved)?;
    let path = percent_encoding::percent_decode_str(&relative)
        .decode_utf8()
        .ok()?
        .replace('\\', "/");
    crate::files::path::components_with(&path, false, crate::files::Access::Manual).ok()?;
    Some(path)
}

fn node_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if let Some(path) = path.strip_prefix("//?/UNC/") {
        format!("//{path}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    }
}

#[cfg(test)]
mod paths {
    use super::path_target;

    #[test]
    fn resolves_captured_roots_on_every_platform() {
        for root in [
            None,
            Some("/remote/project"),
            Some(r"C:\remote\project"),
            Some(r"\\?\C:\remote\project"),
            Some(r"\\?\UNC\server\share\project"),
        ] {
            for (link, expected) in [
                ("report.md", "report.md"),
                ("./report.md", "report.md"),
                ("output/report%20one.pdf", "output/report one.pdf"),
                ("output/%E8%B5%84%E6%96%99.pdf", "output/资料.pdf"),
                ("output/a%2Bb.pdf", "output/a+b.pdf"),
            ] {
                assert_eq!(
                    path_target(link, root).as_deref(),
                    Some(expected),
                    "{root:?}"
                );
            }
        }
    }

    #[test]
    fn confines_absolute_targets() {
        for (root, link) in [
            ("/remote/project", "/remote/project/report.md"),
            ("/remote/project", "file:///remote/project/report.md"),
            (r"\\?\C:\remote\project", r"C:\remote\project\report.md"),
            (r"\\?\C:\remote\project", r"c:\remote\project\report.md"),
            (
                r"\\?\C:\remote\project",
                "file:///c:/remote/project/report.md",
            ),
            (r"c:\remote\project", "file:///C:/remote/project/report.md"),
            (r"\\?\C:\remote\project", r"\\?\C:\remote\project\report.md"),
            (
                r"\\?\UNC\server\share\project",
                r"\\server\share\project\report.md",
            ),
        ] {
            assert_eq!(path_target(link, Some(root)).as_deref(), Some("report.md"));
        }
        for (root, link) in [
            ("/remote/project", "/remote/other/report.md"),
            ("/remote/project", "file://other/remote/project/report.md"),
            (r"\\?\C:\remote\project", r"D:\remote\project\report.md"),
            (
                r"\\?\UNC\server\share\project",
                r"\\other\share\project\report.md",
            ),
        ] {
            assert_eq!(path_target(link, Some(root)), None);
        }
    }

    #[test]
    fn rejects_escapes_and_invalid_targets() {
        for root in [
            None,
            Some("/remote/project"),
            Some(r"\\?\C:\remote\project"),
        ] {
            for link in [
                "",
                "#section",
                "../secret",
                "%2e%2e/secret",
                "%2e%2e%2fsecret",
                "report%00.md",
                "report%0A.md",
                "https://example.test/report.md",
            ] {
                assert_eq!(path_target(link, root), None, "{root:?}: {link}");
            }
        }
        for link in ["/report.md", r"C:\report.md", "file:///report.md"] {
            assert_eq!(path_target(link, None), None);
        }
    }
}
