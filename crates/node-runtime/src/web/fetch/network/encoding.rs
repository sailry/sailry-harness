use encoding_rs::Encoding;
use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};
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

pub(super) fn decode<'a>(bytes: &'a [u8], content_type: &str, html: bool) -> Cow<'a, str> {
    let declaration = charset(content_type)
        .map(str::to_owned)
        .or_else(|| html.then(|| meta(bytes)).flatten());
    let (encoding, body) = if let Some((encoding, size)) = Encoding::for_bom(bytes) {
        (Some(encoding), &bytes[size..])
    } else {
        (
            declaration
                .as_deref()
                .and_then(|label| Encoding::for_label(label.as_bytes())),
            bytes,
        )
    };
    let decoded = if let Some(encoding) = encoding {
        encoding.decode_without_bom_handling(body).0
    } else if let Ok(text) = std::str::from_utf8(body) {
        Cow::Borrowed(text)
    } else {
        encoding_rs::GB18030
            .decode_without_bom_handling_and_without_replacement(body)
            .unwrap_or_else(|| String::from_utf8_lossy(body))
    };
    let invalid = |character: char| {
        character.is_control() && !matches!(character, '\t' | '\n' | '\r' | '\u{c}')
    };
    if decoded.chars().any(invalid) {
        Cow::Owned(
            decoded
                .chars()
                .map(|character| {
                    if invalid(character) {
                        '\u{fffd}'
                    } else {
                        character
                    }
                })
                .collect(),
        )
    } else {
        decoded
    }
}

#[cfg(test)]
mod tests;
