//! Raw HTML in Markdown.
//!
//! Inline tags with a text-style meaning become marks; every other tag stays
//! the literal text it was written as. Blocks keep their exact source. Common
//! README markup becomes native [`layout`] flows, whose images follow the same
//! path as Markdown images. Everything else renders from a sanitized copy: an
//! allowlist of structural tags, attributes and CSS properties, with scripts,
//! event handlers, frames, embedded objects and every resource reference except
//! inline raster `data:` images removed. The rendering surface adds a CSP that
//! blocks scripts and network loads.
use super::doc::Mark;
use ammonia::{Builder, UrlRelative};
use gpui_kit::{Hsla, rgb, rgba};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

pub(crate) mod layout;

/// What one inline tag does to the text around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Role {
    Open(Mark),
    /// Closes the tag open at this position of the pairing stack.
    Close(usize),
    Break,
}

/// Pairs the inline tags of one text. A closing tag with nothing open to
/// close stays literal text, as does every tag without a mapped style.
#[derive(Debug, Default)]
pub(crate) struct Tags {
    open: Vec<&'static str>,
}

impl Tags {
    pub fn accept(&mut self, raw: &str) -> Option<Role> {
        match tag(raw)? {
            Tag::Break => Some(Role::Break),
            Tag::Open(name, mark) => {
                self.open.push(name);
                Some(Role::Open(mark))
            }
            Tag::Close(name) => {
                let at = self.open.iter().rposition(|open| *open == name)?;
                self.open.remove(at);
                Some(Role::Close(at))
            }
        }
    }

    pub fn clear(&mut self) {
        self.open.clear();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Tag {
    Open(&'static str, Mark),
    Close(&'static str),
    Break,
}

/// The canonical name of a tag with a mapped meaning, and the style it
/// applies when it needs nothing from its attributes.
fn mapped(name: &str) -> Option<(&'static str, Option<Mark>)> {
    Some(match name {
        "b" => ("b", Some(Mark::Bold)),
        "strong" => ("strong", Some(Mark::Bold)),
        "i" => ("i", Some(Mark::Italic)),
        "em" => ("em", Some(Mark::Italic)),
        "s" => ("s", Some(Mark::Strike)),
        "del" => ("del", Some(Mark::Strike)),
        "strike" => ("strike", Some(Mark::Strike)),
        "code" => ("code", Some(Mark::Code)),
        "u" => ("u", Some(Mark::Underline)),
        "ins" => ("ins", Some(Mark::Underline)),
        "mark" => ("mark", Some(Mark::Highlight)),
        "kbd" => ("kbd", Some(Mark::Key)),
        "sub" => ("sub", Some(Mark::Subscript)),
        "sup" => ("sup", Some(Mark::Superscript)),
        "a" => ("a", None),
        "span" => ("span", None),
        "br" => ("br", None),
        _ => return None,
    })
}

fn tag(raw: &str) -> Option<Tag> {
    let inner = raw.strip_prefix('<')?.strip_suffix('>')?;
    if let Some(name) = inner.strip_prefix('/') {
        let (name, _) = mapped(&name.trim_end().to_ascii_lowercase())?;
        return (name != "br").then_some(Tag::Close(name));
    }
    let (inner, closed) = match inner.strip_suffix('/') {
        Some(inner) => (inner, true),
        None => (inner, false),
    };
    let end = inner
        .find(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or(inner.len());
    let (name, style) = mapped(&inner[..end].to_ascii_lowercase())?;
    let attributes = attributes(&inner[end..])?;
    if name == "br" {
        return Some(Tag::Break);
    }
    if closed {
        return None;
    }
    let mark = match (name, style) {
        (_, Some(mark)) => mark,
        ("a", None) => Mark::Link(
            attributes
                .into_iter()
                .find(|(key, _)| key == "href")
                .map(|(_, value)| value)
                .filter(|value| !value.is_empty())?,
        ),
        _ => match attributes.as_slice() {
            [(key, value)] if key == "style" => Mark::Color(style_color(value)?),
            _ => return None,
        },
    };
    Some(Tag::Open(name, mark))
}

/// Decoded attributes, or `None` when the tag is not well formed.
fn attributes(mut rest: &str) -> Option<Vec<(String, String)>> {
    let mut result = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Some(result);
        }
        let end = rest
            .find(|ch: char| ch.is_ascii_whitespace() || ch == '=')
            .unwrap_or(rest.len());
        let key = &rest[..end];
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
        {
            return None;
        }
        rest = rest[end..].trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (value, remainder) = match after.chars().next()? {
                quote @ ('"' | '\'') => {
                    let close = after[1..].find(quote)? + 1;
                    (&after[1..close], &after[close + 1..])
                }
                _ => {
                    let close = after
                        .find(|ch: char| ch.is_ascii_whitespace())
                        .unwrap_or(after.len());
                    let value = &after[..close];
                    if value.contains(['"', '\'', '=', '<', '>', '`']) {
                        return None;
                    }
                    (value, &after[close..])
                }
            };
            rest = remainder;
            decode(value)
        } else {
            String::new()
        };
        result.push((key.to_ascii_lowercase(), value));
    }
}

fn decode(value: &str) -> String {
    value
        .replace("&nbsp;", "\u{a0}")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The color of a `style` holding one `color` declaration and nothing else.
fn style_color(style: &str) -> Option<String> {
    let declaration = style.trim().trim_end_matches(';').trim();
    let (property, value) = declaration.split_once(':')?;
    let value = value.trim();
    (property.trim().eq_ignore_ascii_case("color") && color(value).is_some())
        .then(|| value.to_owned())
}

/// Hex, `rgb()`/`rgba()` and common named colors.
pub(crate) fn color(value: &str) -> Option<Hsla> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let expand = |hex: &str| hex.chars().flat_map(|ch| [ch, ch]).collect::<String>();
        let (digits, alpha) = match hex.len() {
            3 => (expand(hex), false),
            4 => (expand(hex), true),
            6 => (hex.to_owned(), false),
            8 => (hex.to_owned(), true),
            _ => return None,
        };
        let number = u32::from_str_radix(&digits, 16).ok()?;
        return Some(if alpha { rgba(number) } else { rgb(number) }.into());
    }
    if let Some(arguments) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<_> = arguments
            .split([',', ' ', '/'])
            .filter(|part| !part.is_empty())
            .collect();
        let (channels, alpha) = match parts.as_slice() {
            [red, green, blue] => ([*red, *green, *blue], None),
            [red, green, blue, alpha] => ([*red, *green, *blue], Some(*alpha)),
            _ => return None,
        };
        let mut number = 0u32;
        for channel in channels {
            number = (number << 8) | u32::from(channel.parse::<u8>().ok()?);
        }
        let alpha = match alpha {
            None => 255,
            Some(alpha) => {
                let alpha = alpha.parse::<f32>().ok()?;
                if !(0.0..=1.0).contains(&alpha) {
                    return None;
                }
                (alpha * 255.0).round() as u32
            }
        };
        return Some(rgba((number << 8) | alpha).into());
    }
    NAMED
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, number)| rgb(*number).into())
}

const NAMED: [(&str, u32); 32] = [
    ("black", 0x000000),
    ("silver", 0xc0c0c0),
    ("gray", 0x808080),
    ("grey", 0x808080),
    ("white", 0xffffff),
    ("maroon", 0x800000),
    ("red", 0xff0000),
    ("purple", 0x800080),
    ("fuchsia", 0xff00ff),
    ("magenta", 0xff00ff),
    ("green", 0x008000),
    ("lime", 0x00ff00),
    ("olive", 0x808000),
    ("yellow", 0xffff00),
    ("navy", 0x000080),
    ("blue", 0x0000ff),
    ("teal", 0x008080),
    ("aqua", 0x00ffff),
    ("cyan", 0x00ffff),
    ("orange", 0xffa500),
    ("pink", 0xffc0cb),
    ("brown", 0xa52a2a),
    ("gold", 0xffd700),
    ("indigo", 0x4b0082),
    ("violet", 0xee82ee),
    ("crimson", 0xdc143c),
    ("coral", 0xff7f50),
    ("tomato", 0xff6347),
    ("darkred", 0x8b0000),
    ("darkgreen", 0x006400),
    ("darkblue", 0x00008b),
    ("darkorange", 0xff8c00),
];

/// The opening and closing spelling of a mark that only HTML can write.
pub(crate) fn spelling(mark: &Mark) -> Option<(String, &'static str)> {
    Some(match mark {
        Mark::Underline => ("<u>".into(), "</u>"),
        Mark::Highlight => ("<mark>".into(), "</mark>"),
        Mark::Key => ("<kbd>".into(), "</kbd>"),
        Mark::Subscript => ("<sub>".into(), "</sub>"),
        Mark::Superscript => ("<sup>".into(), "</sup>"),
        // Only a value that parsed as a color is stored, so it holds no quote.
        Mark::Color(value) => (format!("<span style=\"color: {value}\">"), "</span>"),
        _ => return None,
    })
}

/// Whether a block holds nothing but comments, which a reader does not show.
pub(crate) fn hidden(source: &str) -> bool {
    let mut rest = source.trim();
    while let Some(comment) = rest.strip_prefix("<!--") {
        let Some(end) = comment.find("-->") else {
            return false;
        };
        rest = comment[end + 3..].trim_start();
    }
    rest.is_empty()
}

const TAGS: [&str; 64] = [
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "b",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "main",
    "mark",
    "nav",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "section",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
];

/// Tags dropped together with their content. Everything else outside
/// [`TAGS`] is unwrapped and keeps its text.
const REMOVED: [&str; 19] = [
    "script", "style", "iframe", "frame", "frameset", "object", "embed", "applet", "noscript",
    "template", "svg", "math", "title", "textarea", "select", "audio", "video", "canvas", "button",
];

const STYLE_PROPERTIES: [&str; 76] = [
    "color",
    "background",
    "background-color",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "font-variant",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "text-align",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-style",
    "text-transform",
    "text-indent",
    "text-shadow",
    "white-space",
    "word-break",
    "overflow-wrap",
    "vertical-align",
    "direction",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-color",
    "border-style",
    "border-width",
    "border-radius",
    "border-collapse",
    "border-spacing",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "display",
    "float",
    "clear",
    "box-sizing",
    "list-style",
    "list-style-type",
    "list-style-position",
    "opacity",
    "box-shadow",
    "gap",
    "flex",
    "flex-direction",
    "flex-wrap",
    "justify-content",
    "align-items",
    "align-content",
    "align-self",
    "grid-template-columns",
    "grid-template-rows",
    "grid-column",
    "grid-row",
    "column-count",
    "table-layout",
    "caption-side",
];

/// CSS that could fetch, hide an escape, or reach outside the declaration.
const UNSAFE_STYLE: [&str; 7] = [
    "url(",
    "image",
    "expression",
    "\\",
    "@",
    "/*",
    "javascript:",
];

const IMAGE_TYPES: [&str; 4] = [
    "data:image/png;base64,",
    "data:image/jpeg;base64,",
    "data:image/gif;base64,",
    "data:image/webp;base64,",
];

/// Sanitize a block for rendering. Never written back to the document.
pub fn sanitize(source: &str) -> String {
    let mut builder = cleaner(&[]);
    builder
        .url_schemes(["data"].into_iter().collect())
        .url_relative(UrlRelative::Deny)
        .attribute_filter(filter)
        .clean(source)
        .to_string()
}

/// What the surface rasterizes: nothing for a block native flows show in full.
pub fn raster(source: &str) -> String {
    if layout::layout(source).complete {
        String::new()
    } else {
        sanitize(source)
    }
}

/// The shared allowlist, plus attributes a caller's URL policy covers.
fn cleaner<'a>(references: &[(&'a str, &'a str)]) -> Builder<'a> {
    let mut attributes = HashMap::new();
    for (tag, names) in [
        ("img", "src alt"),
        ("td", "colspan rowspan"),
        ("th", "colspan rowspan"),
        ("col", "span"),
        ("colgroup", "span"),
        ("ol", "start type reversed"),
        ("li", "value"),
        ("font", "size face"),
        ("table", "border cellpadding cellspacing"),
    ]
    .into_iter()
    .chain(references.iter().copied())
    {
        attributes.insert(tag, names.split_whitespace().collect::<HashSet<_>>());
    }
    let mut builder = Builder::empty();
    builder
        .tags(TAGS.into_iter().collect())
        .clean_content_tags(REMOVED.into_iter().collect())
        .generic_attributes(
            [
                "align", "valign", "style", "title", "dir", "lang", "width", "height", "bgcolor",
                "color",
            ]
            .into_iter()
            .collect(),
        )
        .tag_attributes(attributes)
        .link_rel(None)
        .strip_comments(true)
        .filter_style_properties(STYLE_PROPERTIES.into_iter().collect());
    builder
}

fn filter<'a>(tag: &str, name: &str, value: &'a str) -> Option<Cow<'a, str>> {
    if value.len() > super::visuals::MAX_SOURCE_BYTES {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    let allowed = match (tag, name) {
        ("img", "src") => IMAGE_TYPES.iter().any(|kind| lower.starts_with(kind)),
        (_, "style") => !UNSAFE_STYLE.iter().any(|pattern| lower.contains(pattern)),
        (_, "width" | "height" | "colspan" | "rowspan" | "span" | "start" | "value" | "size")
        | ("table", "border" | "cellpadding" | "cellspacing") => {
            !value.is_empty()
                && value.len() <= 8
                && value
                    .trim_end_matches(['%', 'x', 'p'])
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'-')
        }
        (_, "bgcolor" | "color") => color(value).is_some(),
        _ => !lower.contains("javascript:"),
    };
    allowed.then_some(Cow::Borrowed(value))
}

#[cfg(test)]
mod tests;
