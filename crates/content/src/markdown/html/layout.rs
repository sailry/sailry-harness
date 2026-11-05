//! Common README markup as native flows of text and pictures.
//!
//! Scans the serialized output of an allowlist sanitizer, which is well nested
//! with every attribute quoted, so nothing here interprets untrusted markup.
//! Images resolve through the surface's Markdown image path rather than the
//! rasterizing WebView, which can reach neither files nor the network.
use super::{IMAGE_TYPES, attributes, cleaner, decode, filter, mapped};
use crate::markdown::doc::{Mark, MarkSpan, Text};
use ammonia::UrlRelative;
use std::{borrow::Cow, collections::HashMap};

pub type Layouts = HashMap<String, Layout>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub url: String,
    pub alt: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// The enclosing link's address.
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Text(Text),
    Picture(Picture),
}

/// One line of inline content.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub align: Align,
    pub heading: Option<u8>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub flows: Vec<Flow>,
    /// Whether the flows express the whole block. Otherwise they only stand in
    /// until, or instead of, its sanitized picture.
    pub complete: bool,
}

pub fn layout(source: &str) -> Layout {
    if source.len() > crate::markdown::visuals::MAX_SOURCE_BYTES {
        return Layout::default();
    }
    let clean = cleaner(&[("a", "href")])
        .url_schemes(["http", "https", "mailto", "data"].into_iter().collect())
        .url_relative(UrlRelative::PassThrough)
        .attribute_filter(references)
        .clean(source)
        .to_string();
    let mut composer = Composer {
        complete: true,
        ..Composer::default()
    };
    composer.scan(&clean);
    composer.finish_flow();
    Layout {
        flows: composer.flows,
        complete: composer.complete,
    }
}

/// The addresses Markdown images and links accept, and the shared allowlist
/// for everything else. Protocol-relative and other schemes are dropped.
fn references<'a>(tag: &str, name: &str, value: &'a str) -> Option<Cow<'a, str>> {
    let lower = value.trim().to_ascii_lowercase();
    let allowed = match (tag, name) {
        ("img", "src") => {
            remote(&lower)
                || IMAGE_TYPES.iter().any(|kind| lower.starts_with(kind))
                || relative(&lower)
        }
        ("a", "href") => {
            remote(&lower)
                || lower.starts_with("mailto:")
                || (relative(&lower) && !lower.is_empty())
        }
        _ => return filter(tag, name, value),
    };
    allowed.then_some(Cow::Borrowed(value))
}

fn remote(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn relative(url: &str) -> bool {
    !url.starts_with("//")
        && !url.contains('\\')
        && !url
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .contains(':')
}

fn heading(name: &str) -> Option<u8> {
    match name.as_bytes() {
        [b'h', level @ b'1'..=b'6'] => Some(level - b'0'),
        _ => None,
    }
}

fn block(name: &str) -> bool {
    matches!(name, "p" | "div" | "center") || heading(name).is_some()
}

fn aligned(value: &str) -> Option<Align> {
    match value.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Some(Align::Start),
        "center" | "middle" => Some(Align::Center),
        "right" | "end" => Some(Align::End),
        _ => None,
    }
}

/// The alignment of a `style` holding one `text-align` and nothing else.
fn style_align(style: &str) -> Option<Align> {
    let (property, value) = style.trim().trim_end_matches(';').split_once(':')?;
    property
        .trim()
        .eq_ignore_ascii_case("text-align")
        .then(|| aligned(value))
        .flatten()
}

fn length(value: &str) -> Option<f32> {
    let value = value.trim();
    let number = value.strip_suffix("px").unwrap_or(value);
    number
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.)
}

/// Whether flows can honor every attribute a tag kept.
fn honored(name: &str, attributes: &[(String, String)]) -> bool {
    attributes.iter().all(|(key, value)| match key.as_str() {
        "align" | "title" | "lang" => true,
        "href" => name == "a",
        "src" | "alt" | "width" | "height" => name == "img",
        "style" => style_align(value).is_some(),
        _ => false,
    })
}

fn attribute<'a>(attributes: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

struct Context {
    name: String,
    align: Align,
    heading: Option<u8>,
}

struct OpenMark {
    name: &'static str,
    start: usize,
    mark: Mark,
}

#[derive(Default)]
struct Composer {
    flows: Vec<Flow>,
    complete: bool,
    blocks: Vec<Context>,
    marks: Vec<OpenMark>,
    items: Vec<Item>,
    text: Text,
}

impl Composer {
    fn scan(&mut self, mut rest: &str) {
        while let Some(start) = rest.find('<') {
            self.push_text(&decode(&rest[..start]));
            rest = &rest[start..];
            let Some(end) = tag_end(rest) else {
                // The sanitizer always closes its tags; distrust the block if not.
                self.complete = false;
                return;
            };
            self.tag(&rest[1..end]);
            rest = &rest[end + 1..];
        }
        self.push_text(&decode(rest));
    }

    fn tag(&mut self, tag: &str) {
        if let Some(name) = tag.strip_prefix('/') {
            self.close(name.trim());
            return;
        }
        let end = tag
            .find(|ch: char| ch.is_ascii_whitespace())
            .unwrap_or(tag.len());
        let name = &tag[..end];
        let Some(attributes) = attributes(&tag[end..]) else {
            self.complete = false;
            return;
        };
        self.complete &= honored(name, &attributes);
        if name == "img" {
            self.picture(&attributes);
        } else if name == "br" {
            self.finish_flow();
        } else if block(name) {
            self.open_block(name, &attributes);
        } else if let Some((canonical, style)) = mapped(name) {
            let mark = match canonical {
                "a" => attribute(&attributes, "href").map(|url| Mark::Link(url.to_owned())),
                _ => style,
            };
            if let Some(mark) = mark {
                self.marks.push(OpenMark {
                    name: canonical,
                    start: self.text.text.len(),
                    mark,
                });
            }
        } else {
            // Structure flows cannot show: keep its content on separate lines.
            self.complete = false;
            self.finish_flow();
        }
    }

    fn close(&mut self, name: &str) {
        if block(name) {
            self.finish_flow();
            if let Some(at) = self.blocks.iter().rposition(|open| open.name == name) {
                self.blocks.truncate(at);
            }
        } else if let Some((canonical, _)) = mapped(name) {
            if let Some(at) = self.marks.iter().rposition(|open| open.name == canonical) {
                let open = self.marks.remove(at);
                self.text.marks.push(MarkSpan {
                    range: open.start..self.text.text.len(),
                    mark: open.mark,
                });
            }
        } else {
            self.finish_flow();
        }
    }

    fn open_block(&mut self, name: &str, attributes: &[(String, String)]) {
        self.finish_flow();
        let inherited = self.blocks.last();
        let align = attribute(attributes, "align")
            .and_then(aligned)
            .or_else(|| attribute(attributes, "style").and_then(style_align))
            .or((name == "center").then_some(Align::Center))
            .or(inherited.map(|open| open.align))
            .unwrap_or_default();
        let heading = heading(name).or(inherited.and_then(|open| open.heading));
        self.blocks.push(Context {
            name: name.to_owned(),
            align,
            heading,
        });
    }

    fn picture(&mut self, attributes: &[(String, String)]) {
        let alt = attribute(attributes, "alt").unwrap_or_default();
        let url = attribute(attributes, "src").filter(|url| !url.is_empty());
        let Some(url) = url.filter(|url| !url.to_ascii_lowercase().starts_with("data:")) else {
            // Inline data keeps rendering on the sanitized picture.
            self.complete &= url.is_none();
            self.push_text(alt);
            return;
        };
        self.flush_text();
        let link = self.marks.iter().rev().find_map(|open| match &open.mark {
            Mark::Link(url) => Some(url.clone()),
            _ => None,
        });
        self.items.push(Item::Picture(Picture {
            url: url.to_owned(),
            alt: alt.to_owned(),
            width: attribute(attributes, "width").and_then(length),
            height: attribute(attributes, "height").and_then(length),
            link,
        }));
    }

    /// Appends text, collapsing white space as HTML does. No-break spaces stay.
    fn push_text(&mut self, value: &str) {
        for ch in value.chars() {
            if !ch.is_ascii_whitespace() {
                self.text.text.push(ch);
            } else if !self.text.text.is_empty() && !self.text.text.ends_with(' ') {
                self.text.text.push(' ');
            }
        }
    }

    /// Ends the current text; marks still open continue on the next one.
    fn flush_text(&mut self) {
        let mut text = std::mem::take(&mut self.text);
        for open in &mut self.marks {
            text.marks.push(MarkSpan {
                range: open.start..text.text.len(),
                mark: open.mark.clone(),
            });
            open.start = 0;
        }
        text.text.truncate(text.text.trim_end_matches(' ').len());
        let len = text.text.len();
        text.marks.retain_mut(|span| {
            span.range.end = span.range.end.min(len);
            span.range.start < span.range.end
        });
        // Outermost first, as the inline renderer expects.
        text.marks
            .sort_by_key(|span| (span.range.start, std::cmp::Reverse(span.range.end)));
        if !text.text.trim().is_empty() {
            self.items.push(Item::Text(text));
        }
    }

    fn finish_flow(&mut self) {
        self.flush_text();
        if self.items.is_empty() {
            return;
        }
        let context = self.blocks.last();
        self.flows.push(Flow {
            align: context.map(|open| open.align).unwrap_or_default(),
            heading: context.and_then(|open| open.heading),
            items: std::mem::take(&mut self.items),
        });
    }
}

/// The closing `>` of the tag `rest` starts with; attributes may contain one.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (at, ch) in rest.char_indices() {
        match (quote, ch) {
            (None, '>') => return Some(at),
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), _) if open == ch => quote = None,
            _ => {}
        }
    }
    None
}
