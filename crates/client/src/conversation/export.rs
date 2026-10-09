//! Portable exports hydrate canonical history through the same authenticated Client.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, SessionId,
    attachment::Attachment,
    conversation::{Page, Part},
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Html,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
        }
    }
}

#[derive(Debug)]
pub struct Document {
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub images: Vec<sailry_protocol::conversation::Image>,
}

impl Client {
    /// Captures committed history. Unsent queue items and streaming drafts are excluded.
    pub async fn export_conversation(
        &self,
        session: SessionId,
        title: &str,
        format: Format,
        assets: &str,
        cancel: &CancellationToken,
    ) -> Result<Document, Fault> {
        let mut history = tokio::select! { _ = cancel.cancelled() => return Err(cancelled()), result = self.read_conversation(session, None, 50) => result? };
        let mut bytes = serde_json::to_vec(&history.page)
            .map_err(|error| Fault::invalid(error.to_string()))?
            .len();
        let mut seen = BTreeSet::new();
        while let Some(cursor) = history.page.next_before {
            if !seen.insert(cursor) {
                return Err(Fault::invalid("conversation export cursor did not advance"));
            }
            let older = tokio::select! { _ = cancel.cancelled() => return Err(cancelled()), result = self.read_conversation(session, Some(cursor), 50) => result? };
            bytes = bytes.saturating_add(
                serde_json::to_vec(&older.page)
                    .map_err(|error| Fault::invalid(error.to_string()))?
                    .len(),
            );
            if bytes > MAX_BYTES {
                return Err(Fault::invalid("conversation export exceeds 64 MiB"));
            }
            let next = older.page.next_before;
            super::history::merge(&mut history.page, older.page, true)?;
            history.page.next_before = next;
        }
        render(&history.page, title, format, assets)
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "conversation export was cancelled")
}

pub fn attachment_name(attachment: &Attachment) -> String {
    let name: String = attachment
        .spec
        .name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    format!(
        "{}-{}",
        attachment.id,
        if name.is_empty() { "attachment" } else { &name }
    )
}

fn render(page: &Page, title: &str, format: Format, assets: &str) -> Result<Document, Fault> {
    if assets.is_empty()
        || assets.chars().any(|character| character.is_control())
        || assets.contains(['\\', ':', '<', '>'])
        || assets
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(Fault::invalid(
            "export attachment directory must be a relative path",
        ));
    }
    let mut markdown = format!("# {}\n\n", heading(title));
    let mut attachments = BTreeMap::new();
    let mut images = BTreeMap::new();
    for entry in &page.entries {
        markdown.push_str(&format!("## {}\n\n", heading(&entry.author)));
        for part in &entry.parts {
            match part {
                Part::Text(text) => {
                    markdown.push_str(text);
                    markdown.push_str("\n\n");
                }
                Part::Thinking(text) | Part::Compaction(text) => {
                    fenced(&mut markdown, text, "text");
                }
                Part::Reference(reference) => {
                    markdown.push_str(&format!("> {}\n\n", heading(&reference.label)));
                }
                Part::Attachment(attachment) => {
                    attachment_link(&mut markdown, &mut attachments, attachment, assets, false)?
                }
                Part::Image(image) => {
                    attachment_link(
                        &mut markdown,
                        &mut attachments,
                        &image.attachment,
                        assets,
                        true,
                    )?;
                    images.insert(image.attachment.id, image.clone());
                }
                Part::ToolCall {
                    name,
                    arguments,
                    display,
                    ..
                } => {
                    markdown.push_str(&format!("> `{}`", heading(name).replace('`', "\\`")));
                    if let Some(text) = display
                        .as_ref()
                        .filter(|display| display.valid())
                        .and_then(|display| display.input.as_ref())
                        .and_then(|input| input.summary.as_ref())
                        .and_then(|field| field.read(arguments))
                    {
                        markdown.push_str(": ");
                        markdown.push_str(&heading(text));
                    }
                    markdown.push_str("\n\n");
                }
                Part::ToolResult {
                    name,
                    result,
                    images: inline,
                    ..
                } => {
                    markdown.push_str(&format!("> `{}`\n\n", heading(name).replace('`', "\\`")));
                    let text = serde_json::to_string_pretty(result)
                        .map_err(|error| Fault::invalid(error.to_string()))?;
                    if text.chars().count() <= 2000 {
                        fenced(&mut markdown, &text, "json");
                    } else {
                        fenced(
                            &mut markdown,
                            &format!("{}…", text.chars().take(2000).collect::<String>()),
                            "json",
                        );
                    }
                    for image in inline {
                        attachment_link(
                            &mut markdown,
                            &mut attachments,
                            &image.attachment,
                            assets,
                            true,
                        )?;
                        images.insert(image.attachment.id, image.clone());
                    }
                }
                Part::Resource(value) => fenced(&mut markdown, &value.to_string(), "json"),
            }
            if markdown.len() > MAX_BYTES {
                return Err(Fault::invalid("conversation export exceeds 64 MiB"));
            }
        }
        for citation in &entry.citations {
            if safe_url(&citation.uri) {
                markdown.push_str(&format!(
                    "[{}](<{}>)\n\n",
                    label(citation.title.as_deref().unwrap_or(&citation.uri)),
                    citation.uri.replace(['<', '>'], "")
                ));
            }
        }
    }
    let content = match format {
        Format::Markdown => markdown,
        Format::Html => html(title, &markdown),
    };
    if content.len() > MAX_BYTES {
        return Err(Fault::invalid("conversation export exceeds 64 MiB"));
    }
    Ok(Document {
        content,
        attachments: attachments.into_values().collect(),
        images: images.into_values().collect(),
    })
}

fn attachment_link(
    output: &mut String,
    attachments: &mut BTreeMap<sailry_protocol::AttachmentId, Attachment>,
    attachment: &Attachment,
    assets: &str,
    image: bool,
) -> Result<(), Fault> {
    if attachments
        .get(&attachment.id)
        .is_some_and(|previous| previous != attachment)
    {
        return Err(Fault::invalid("export attachment identity changed"));
    }
    attachments.insert(attachment.id, attachment.clone());
    output.push_str(&format!(
        "{}[{}](<{assets}/{}>)\n\n",
        if image { "!" } else { "" },
        label(&attachment.spec.name),
        attachment_name(attachment)
    ));
    Ok(())
}

fn heading(value: &str) -> String {
    value
        .lines()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['#', '[', ']', '<', '>'], "")
}
fn label(value: &str) -> String {
    value
        .lines()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('<', "\\<")
        .replace('>', "\\>")
}
fn fenced(output: &mut String, text: &str, language: &str) {
    let count = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(count);
    output.push_str(&format!("{fence}{language}\n{text}\n{fence}\n\n"));
}
fn safe_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none()
    })
}
fn html(title: &str, markdown: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let parser = Parser::new_ext(
        markdown,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: if safe_url(&dest_url) || !dest_url.contains(':') {
                dest_url
            } else {
                "#".into()
            },
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: if safe_url(&dest_url) || !dest_url.contains(':') {
                dest_url
            } else {
                "#".into()
            },
            title,
            id,
        }),
        event => event,
    });
    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, parser);
    let title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>body{{max-width:900px;margin:3rem auto;padding:0 1rem;font:16px/1.6 system-ui}}pre{{overflow:auto;padding:1rem;background:#f5f5f5}}img{{max-width:100%}}table{{border-collapse:collapse}}td,th{{padding:.5rem;border:1px solid #ccc}}blockquote{{margin-left:0;padding-left:1rem;border-left:3px solid #ccc}}</style></head><body>{body}</body></html>"
    )
}
