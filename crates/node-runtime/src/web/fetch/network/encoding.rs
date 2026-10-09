use super::unavailable;
use encoding_rs::Encoding;
use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
use sailry_protocol::Fault;
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

fn charset(value: &str) -> Option<&str> {
    value.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches(['\'', '"']))
    })
}

fn meta(bytes: &[u8]) -> Option<String> {
    // Charset declarations are ASCII and must appear in the first 1024 bytes.
    // This bounded view is used only for HTML parsing, never as returned text.
    let head: String = bytes
        .iter()
        .take(1024)
        .map(|byte| char::from(*byte))
        .collect();
    let label = Arc::new(Mutex::new(None));
    let captured = label.clone();
    let converter = HtmlToMarkdown::builder()
        .add_handler(vec!["head"], |handlers: &dyn Handlers, element: Element| {
            handlers.walk_children(element.node);
            None
        })
        .add_handler(vec!["meta"], move |_: &dyn Handlers, element: Element| {
            let attribute = |name: &str| {
                element
                    .attrs
                    .iter()
                    .find(|attribute| attribute.name.local.as_ref().eq_ignore_ascii_case(name))
                    .map(|attribute| attribute.value.as_ref())
            };
            let found = attribute("charset").or_else(|| {
                attribute("http-equiv")
                    .filter(|value| value.eq_ignore_ascii_case("content-type"))
                    .and_then(|_| attribute("content").and_then(charset))
            });
            if let Some(found) = found.filter(|value| value.is_ascii() && value.len() <= 100) {
                let mut label = captured.lock().expect("web charset lock is healthy");
                if label.is_none() {
                    *label = Some(found.to_owned());
                }
            }
            None
        })
        .build();
    let _ = converter.convert(&head);
    let mut label = label.lock().expect("web charset lock is healthy");
    label.take()
}

pub(super) fn decode<'a>(
    bytes: &'a [u8],
    content_type: &str,
    html: bool,
) -> Result<Cow<'a, str>, Fault> {
    let declaration = charset(content_type)
        .map(str::to_owned)
        .or_else(|| html.then(|| meta(bytes)).flatten());
    let (encoding, body) = if let Some((encoding, size)) = Encoding::for_bom(bytes) {
        (Some(encoding), &bytes[size..])
    } else if let Some(label) = declaration {
        (
            Some(
                Encoding::for_label(label.as_bytes())
                    .ok_or_else(|| unavailable("web response declares an unsupported charset"))?,
            ),
            bytes,
        )
    } else {
        (None, bytes)
    };
    let decoded = if let Some(encoding) = encoding {
        encoding
            .decode_without_bom_handling_and_without_replacement(body)
            .ok_or_else(|| unavailable("web response contains invalid text for its charset"))?
    } else {
        crate::files::text::decode(body)
            .map_err(|_| unavailable("web response contains unsupported text"))?
    };
    if decoded.chars().any(|character| {
        character.is_control() && !matches!(character, '\t' | '\n' | '\r' | '\u{c}')
    }) {
        return Err(unavailable("web response contains binary content"));
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests;
