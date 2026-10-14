//! Portable exports hydrate canonical history through the same authenticated Client.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, SessionId, TurnId,
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
        self.export_selection(session, None, title, format, assets, cancel)
            .await
    }

    /// Exports the user message and final reply belonging to the requested turn.
    pub async fn export_reply(
        &self,
        session: SessionId,
        turn: TurnId,
        title: &str,
        format: Format,
        assets: &str,
        cancel: &CancellationToken,
    ) -> Result<Document, Fault> {
        self.export_selection(session, Some(turn), title, format, assets, cancel)
            .await
    }

    async fn export_selection(
        &self,
        session: SessionId,
        turn: Option<TurnId>,
        title: &str,
        format: Format,
        assets: &str,
        cancel: &CancellationToken,
    ) -> Result<Document, Fault> {
        let mut history = tokio::select! { _ = cancel.cancelled() => return Err(cancelled()), result = self.read_conversation(session, None, 50) => result? };
        let mut bytes = serde_json::to_vec(&history.page)
            .map_err(|error| Fault::invalid(error.to_string()))?
            .len();
        if bytes > MAX_BYTES {
            return Err(Fault::invalid("conversation export exceeds 64 MiB"));
        }
        let mut seen = BTreeSet::new();
        while let Some(cursor) = history.page.next_before {
            if turn.is_some_and(|turn| history.page.entries.iter().any(|entry| entry.turn == turn))
            {
                break;
            }
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
        if let Some(turn) = turn {
            history.page.entries.retain(|entry| entry.turn == turn);
            if history.page.entries.is_empty() {
                return Err(Fault::invalid("Reply is unavailable"));
            }
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
    let mut markdown = String::new();
    let mut messages = String::new();
    let mut attachments = BTreeMap::new();
    let mut images = BTreeMap::new();
    // Match the transcript's final reply boundary: work before the last tool,
    // reasoning or compaction part is not part of the published answer.
    let mut boundaries = BTreeMap::new();
    for entry in &page.entries {
        if entry.author == "user" {
            continue;
        }
        for (index, part) in entry.parts.iter().enumerate() {
            if super::reply::is_work(part) {
                boundaries.insert(entry.turn, (entry.sequence, index));
            }
        }
    }
    let conversation = page.entries.iter().any(|entry| entry.author == "user");
    for entry in &page.entries {
        let user = entry.author == "user";
        // Automatic controller input is execution context, not user-authored speech.
        if user
            && page
                .runs
                .iter()
                .any(|run| run.turn == entry.turn && run.continuation.is_some())
        {
            continue;
        }
        let mut body = String::new();
        let mut text_run = false;
        for (index, part) in entry.parts.iter().enumerate() {
            if !user
                && boundaries
                    .get(&entry.turn)
                    .is_some_and(|boundary| (entry.sequence, index) <= *boundary)
            {
                continue;
            }
            match part {
                Part::Text(text) => {
                    body.push_str(text);
                    text_run = true;
                }
                Part::Attachment(attachment) => {
                    finish_text(&mut body, &mut text_run);
                    attachment_link(&mut body, &mut attachments, attachment, assets, false)?
                }
                Part::Image(image) => {
                    finish_text(&mut body, &mut text_run);
                    attachment_link(&mut body, &mut attachments, &image.attachment, assets, true)?;
                    images.insert(image.attachment.id, image.clone());
                }
                _ => finish_text(&mut body, &mut text_run),
            }
        }
        finish_text(&mut body, &mut text_run);
        if body.is_empty() {
            continue;
        }
        if conversation {
            markdown.push_str(&format!(
                "## {}\n\n",
                entry.author.replace(['#', '<', '>', '\n', '\r'], "")
            ));
        }
        for citation in &entry.citations {
            if safe_url(&citation.uri) {
                body.push_str(&format!(
                    "[{}](<{}>)\n\n",
                    label(citation.title.as_deref().unwrap_or(&citation.uri)),
                    citation.uri.replace(['<', '>'], "")
                ));
            }
        }
        markdown.push_str(&body);
        if format == Format::Html {
            messages.push_str(&format!(
                "<section class=\"message {}\">{}</section>",
                if user { "user" } else { "assistant" },
                html_body(&body),
            ));
        }
        if markdown.len() > MAX_BYTES || messages.len() > MAX_BYTES {
            return Err(Fault::invalid("conversation export exceeds 64 MiB"));
        }
    }
    let content = match format {
        Format::Markdown => markdown,
        Format::Html => html(title, &messages),
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

fn finish_text(output: &mut String, active: &mut bool) {
    if std::mem::take(active) {
        output.push_str("\n\n");
    }
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
fn safe_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none()
    })
}
fn html_body(markdown: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let markdown = math_delimiters(markdown);
    let parser = Parser::new_ext(
        &markdown,
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_MATH,
    )
    .map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        Event::InlineMath(value) => Event::Html(
            format!(
                "<span class=\"math inline\">\\({}\\)</span>",
                escape(&value)
            )
            .into(),
        ),
        Event::DisplayMath(value) => Event::Html(
            format!("<div class=\"math display\">\\[{}\\]</div>", escape(&value)).into(),
        ),
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
    body
}

fn html(title: &str, body: &str) -> String {
    let title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>body{{max-width:900px;margin:3rem auto;padding:0 1rem;font:16px/1.6 system-ui}}.message{{margin:24px 0;overflow-wrap:anywhere}}.user{{width:fit-content;max-width:85%;margin-left:auto;padding:10px 18px;border-radius:16px;background:#292929;color:#fff}}.user>:first-child{{margin-top:0}}.user>:last-child{{margin-bottom:0}}.user a{{color:inherit}}.user pre{{color:#20242b}}pre{{overflow:auto;padding:1rem;background:#f5f5f5}}img{{max-width:100%}}table{{border-collapse:collapse}}td,th{{padding:.5rem;border:1px solid #ccc}}blockquote{{margin-left:0;padding-left:1rem;border-left:3px solid #ccc}}</style></head><body>{body}</body></html>"
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn math_delimiters(source: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag};
    let protected: Vec<_> = Parser::new(source)
        .into_offset_iter()
        .filter_map(|(event, span)| {
            matches!(
                event,
                Event::Code(_)
                    | Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
                    | Event::Html(_)
                    | Event::InlineHtml(_)
            )
            .then_some(span)
        })
        .collect();
    let mut output = String::new();
    let mut cursor = 0;
    while let Some(at) = source[cursor..].find('\\').map(|offset| cursor + offset) {
        output.push_str(&source[cursor..at]);
        let delimiter = source.as_bytes().get(at + 1).copied();
        let close = match delimiter {
            Some(b'(') => Some("\\)"),
            Some(b'[') => Some("\\]"),
            _ => None,
        };
        let end =
            close.and_then(|close| source[at + 2..].find(close).map(|offset| at + 2 + offset));
        if let Some(end) = end.filter(|end| {
            !protected
                .iter()
                .any(|span| span.start < *end + 2 && span.end > at)
                && (at == 0 || source.as_bytes()[at - 1] != b'\\')
        }) {
            let marker = if delimiter == Some(b'[') { "$$" } else { "$" };
            output.push_str(marker);
            output.push_str(&source[at + 2..end]);
            output.push_str(marker);
            cursor = end + 2;
        } else {
            output.push('\\');
            cursor = at + 1;
        }
    }
    output.push_str(&source[cursor..]);
    output
}
