//! Bounded authored source; declarative documents use the package sanitizer.
use ammonia::{Builder, UrlRelative};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_protocol::{Fault, artifact::MAX_HTML_BYTES, chart::Chart};
use std::collections::{HashMap, HashSet};

pub(crate) fn preview(source: &str) -> Result<(String, Vec<String>), Fault> {
    if source.trim().is_empty() || source.len() > MAX_HTML_BYTES || source.contains('\0') {
        return Err(Fault::invalid(
            "artifact HTML is empty or exceeds its limit",
        ));
    }
    Ok((source.to_owned(), Vec::new()))
}

pub(crate) fn sanitize(
    source: &str,
    allowed_classes: &[String],
) -> Result<(String, Vec<String>), Fault> {
    if source.trim().is_empty() || source.len() > MAX_HTML_BYTES || source.contains('\0') {
        return Err(Fault::invalid(
            "artifact HTML is empty or exceeds its limit",
        ));
    }
    let tags="section h1 h2 h3 h4 p br hr strong em code pre blockquote ul ol li table thead tbody tr th td caption details summary span div a img figure figcaption form fieldset legend label input textarea select option button".split_whitespace().collect();
    let mut attributes = HashMap::new();
    for (tag, names) in [
        ("a", "href"),
        ("img", "src alt"),
        ("th", "colspan rowspan"),
        ("td", "colspan rowspan"),
        ("details", "open"),
        ("form", "data-name"),
        ("fieldset", "data-name data-required"),
        ("label", "for"),
        ("input", "type name value placeholder checked"),
        ("textarea", "name placeholder rows"),
        ("select", "name multiple"),
        ("option", "value selected"),
        ("button", "type"),
    ] {
        attributes.insert(tag, names.split_whitespace().collect::<HashSet<_>>());
    }
    let allowed_classes = allowed_classes.to_vec();
    let cleaned = Builder::empty()
        .tags(tags)
        .clean_content_tags(
            [
                "script", "style", "iframe", "object", "embed", "link", "meta", "base", "svg",
                "math",
            ]
            .into_iter()
            .collect(),
        )
        .generic_attributes(
            ["class", "id", "data-title", "data-kind"]
                .into_iter()
                .collect(),
        )
        .tag_attributes(attributes)
        .url_schemes(["data"].into_iter().collect())
        .url_relative(UrlRelative::PassThrough)
        .link_rel(None)
        .attribute_filter(move |tag, name, value| {
            if value.len() > MAX_HTML_BYTES {
                return None;
            }
            match (tag, name) {
                ("a", "href") => value.starts_with('#').then(|| value.into()),
                ("img", "src") => image(value).then(|| value.into()),
                ("input", "type") => ["radio", "checkbox", "text"]
                    .contains(&value)
                    .then(|| value.into()),
                ("button", "type") => (value == "submit").then(|| value.into()),
                (_, "class") => {
                    let classes = value
                        .split_whitespace()
                        .filter(|class| allowed_classes.iter().any(|allowed| allowed == class))
                        .collect::<Vec<_>>()
                        .join(" ");
                    (!classes.is_empty()).then(|| classes.into())
                }
                (_, "id" | "for") => identifier(value).then(|| value.into()),
                (_, "name" | "data-name") => field(value).then(|| value.into()),
                (_, "colspan" | "rowspan" | "rows") => value
                    .parse::<u16>()
                    .ok()
                    .filter(|value| (1..=100).contains(value))
                    .map(|_| value.into()),
                _ => Some(value.into()),
            }
        })
        .clean(source)
        .to_string();
    let mut warnings = Vec::new();
    if cleaned != source {
        warnings.push("HTML was normalized and unsupported markup was removed".into());
    }
    let html = charts(&cleaned, &mut warnings);
    if html.trim().is_empty() || html.len() > MAX_HTML_BYTES {
        return Err(Fault::invalid(
            "artifact HTML is empty or exceeds its limit after sanitization",
        ));
    }
    Ok((html, warnings))
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

fn field(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn image(value: &str) -> bool {
    if value.len() > 256 * 1024 {
        return false;
    }
    let Some((header, payload)) = value.split_once(',') else {
        return false;
    };
    let Some(kind) = header
        .strip_prefix("data:image/")
        .and_then(|value| value.strip_suffix(";base64"))
    else {
        return false;
    };
    if !["png", "jpeg", "gif", "webp"].contains(&kind) {
        return false;
    }
    let Ok(bytes) = STANDARD.decode(payload) else {
        return false;
    };
    !bytes.is_empty()
        && file_format::FileFormat::from_bytes(&bytes).media_type() == format!("image/{kind}")
}

/// Ammonia escapes literal '<' and '>' in text and attributes, so these
/// boundaries are unambiguous. HTML5 tokenization decodes entities and rejects
/// nested markup rather than interpreting an incomplete JSON text projection.
fn charts(html: &str, warnings: &mut Vec<String>) -> String {
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;
    while let Some(start) = remaining.find("<pre") {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let Some(open) = remaining.find('>') else {
            break;
        };
        let Some(close) = remaining[open + 1..]
            .find("</pre>")
            .map(|index| open + 1 + index)
        else {
            break;
        };
        let opening = &remaining[..open + 1];
        let mut cleaned = opening.to_owned();
        if let Some((start, end)) = class_value(opening) {
            let classes: Vec<_> = opening[start..end].split_whitespace().collect();
            if classes.contains(&"chart") {
                let text = text(&remaining[open + 1..close]);
                if text.as_deref().and_then(Chart::parse).is_none() {
                    let keep = classes
                        .into_iter()
                        .filter(|class| *class != "chart")
                        .collect::<Vec<_>>()
                        .join(" ");
                    cleaned.replace_range(start..end, &keep);
                    if !warnings
                        .iter()
                        .any(|warning| warning == "Invalid chart data was kept as plain text")
                    {
                        warnings.push("Invalid chart data was kept as plain text".into());
                    }
                }
            }
        }
        result.push_str(&cleaned);
        remaining = &remaining[open + 1..];
    }
    result.push_str(remaining);
    result
}

fn class_value(opening: &str) -> Option<(usize, usize)> {
    let bytes = opening.as_bytes();
    let mut quoted = false;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            quoted = !quoted;
        } else if !quoted
            && index > 0
            && bytes[index - 1].is_ascii_whitespace()
            && bytes[index..].starts_with(b"class=\"")
        {
            let start = index + 7;
            let end = start + bytes[start..].iter().position(|byte| *byte == b'"')?;
            return Some((start, end));
        }
    }
    None
}

fn text(html: &str) -> Option<String> {
    use html5ever::{
        buffer_queue::BufferQueue,
        tendril::StrTendril,
        tokenizer::{Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
    };
    struct Sink {
        value: std::cell::RefCell<String>,
        valid: std::cell::Cell<bool>,
    }
    impl TokenSink for Sink {
        type Handle = ();
        fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
            match token {
                Token::CharacterTokens(value) => self.value.borrow_mut().push_str(&value),
                Token::EOFToken | Token::ParseError(_) => {}
                _ => self.valid.set(false),
            }
            TokenSinkResult::Continue
        }
    }
    let tokenizer = Tokenizer::new(
        Sink {
            value: Default::default(),
            valid: std::cell::Cell::new(true),
        },
        TokenizerOpts::default(),
    );
    let buffer = BufferQueue::default();
    buffer.push_back(StrTendril::from(html));
    let _ = tokenizer.feed(&buffer);
    tokenizer.end();
    tokenizer
        .sink
        .valid
        .get()
        .then(|| tokenizer.sink.value.borrow().clone())
}

#[cfg(test)]
#[path = "artifact/tests.rs"]
mod tests;
