//! Byte-preserving patches between decoded document text and Markdown source.
//! CommonMark offsets describe syntax, not displayed cursor offsets. Saving
//! never serializes the complete document or normalizes unrelated source.
mod spelling;

use crate::content::markdown::{
    Block, BlockKind, Cursor, Doc, Part,
    doc::{Container, Text},
    parse::parse,
    parse_ranges,
};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Debug)]
struct Character {
    text: Range<usize>,
    raw: Range<usize>,
}

#[derive(Clone, Debug, Default)]
struct Span {
    value: String,
    characters: Vec<Character>,
    syntax: Option<Range<usize>>,
    prefix: Option<String>,
}

impl Span {
    fn syntax(&mut self, range: Range<usize>) {
        self.syntax = Some(match self.syntax.take() {
            Some(old) => old.start.min(range.start)..old.end.max(range.end),
            None => range,
        });
    }

    fn push(&mut self, value: &str, mut raw: Range<usize>, source: &str, code: bool) {
        self.syntax(raw.clone());
        if code {
            let delimiter = source[raw.clone()]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count();
            raw = raw.start + delimiter..raw.end.saturating_sub(delimiter);
        } else if raw.start > 0
            && source[..raw.start]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 1
        {
            raw.start -= 1;
        }
        let spelling = &source[raw.clone()];
        let mut offset = 0;
        for (at, ch) in value.char_indices() {
            let start = self.value.len() + at;
            let matched = spelling[offset..]
                .char_indices()
                .find_map(|(skip, candidate)| {
                    (candidate == ch || code && ch == ' ' && matches!(candidate, '\n' | '\r'))
                        .then_some((skip, candidate.len_utf8()))
                });
            let mapped = match matched {
                Some((skip, length))
                    if spelling[offset..offset + skip]
                        .chars()
                        .all(|ch| ch.is_whitespace() || ch == '\\') =>
                {
                    let start = offset;
                    offset += skip + length;
                    raw.start + start..raw.start + offset
                }
                _ => raw.clone(),
            };
            self.characters.push(Character {
                text: start..start + ch.len_utf8(),
                raw: mapped,
            });
        }
        self.value.push_str(value);
    }

    fn align(&self, value: &str) -> Option<Self> {
        let mut result = Self {
            syntax: self.syntax.clone(),
            prefix: self.prefix.clone(),
            ..Self::default()
        };
        let mut at = 0;
        for ch in value.chars() {
            let found = self
                .value
                .get(at..)?
                .char_indices()
                .find_map(|(skip, candidate)| {
                    (candidate == ch || ch == ' ' && candidate == '\n').then_some(at + skip)
                })?;
            if !self.value[at..found].chars().all(char::is_whitespace) {
                return None;
            }
            let mapped = self
                .characters
                .iter()
                .find(|entry| entry.text.start == found)?;
            let start = result.value.len();
            result.value.push(ch);
            result.characters.push(Character {
                text: start..result.value.len(),
                raw: mapped.raw.clone(),
            });
            at = mapped.text.end;
        }
        Some(result)
    }

    fn align_live(&self, value: &str, source: &str) -> Option<Self> {
        if let Some(aligned) = self.align(value) {
            return Some(aligned);
        }
        // The live document retains typed or pasted edge spaces that CommonMark
        // omits. Map those original bytes as well so the next edit stays local.
        let visible = value.trim_matches([' ', '\t']);
        let leading = value.len() - value.trim_start_matches([' ', '\t']).len();
        let trailing = &value[leading + visible.len()..];
        let mut aligned = self.align(visible)?;
        let syntax = aligned.syntax.clone()?;
        let start = syntax.start.checked_sub(leading)?;
        if source[start..syntax.start] != value[..leading]
            || !source[syntax.end..].starts_with(trailing)
        {
            return None;
        }
        if leading > 0 {
            let mut prefix = Self::default();
            prefix.push(&value[..leading], start..syntax.start, source, false);
            for character in &mut aligned.characters {
                character.text.start += leading;
                character.text.end += leading;
            }
            prefix.value.push_str(&aligned.value);
            prefix.characters.extend(aligned.characters);
            prefix.syntax(syntax.clone());
            aligned = prefix;
        }
        if !trailing.is_empty() {
            aligned.push(
                trailing,
                syntax.end..syntax.end + trailing.len(),
                source,
                false,
            );
        }
        Some(aligned)
    }

    fn offset(&self, at: usize) -> Option<usize> {
        if at == 0 {
            self.characters
                .first()
                .map(|entry| entry.raw.start)
                .or_else(|| self.syntax.as_ref().map(|range| range.start))
        } else {
            self.characters
                .iter()
                .find(|entry| entry.text.end == at)
                .map(|entry| entry.raw.end)
        }
    }

    fn raw_range(&self, range: Range<usize>) -> Option<Range<usize>> {
        if range.is_empty() {
            let at = self.offset(range.start)?;
            return Some(at..at);
        }
        let entries: Vec<_> = self
            .characters
            .iter()
            .filter(|entry| entry.text.start >= range.start && entry.text.end <= range.end)
            .collect();
        let first = entries.first()?;
        let last = entries.last()?;
        if first.text.start != range.start
            || last.text.end != range.end
            || entries
                .windows(2)
                .any(|pair| pair[0].raw.end < pair[1].raw.start)
        {
            return None;
        }
        Some(first.raw.start..last.raw.end)
    }
}

#[derive(Clone, Debug)]
struct Leaf {
    range: Range<usize>,
    parts: Vec<(Part, Span)>,
}

#[derive(Clone, Debug)]
pub(super) struct Map {
    leaves: Vec<Leaf>,
}

#[derive(Clone, Debug)]
pub(super) struct Patch {
    pub range: Range<usize>,
    pub text: String,
}

impl Map {
    pub fn new(source: &str, doc: &Doc) -> Self {
        let partitions = parse_ranges(source).block_ranges;
        let mut leaves: Vec<_> = doc
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| Leaf {
                range: partitions
                    .get(index)
                    .cloned()
                    .unwrap_or(source.len()..source.len()),
                parts: block
                    .parts()
                    .into_iter()
                    .map(|part| (part, Span::default()))
                    .collect(),
            })
            .collect();
        let mut extents = vec![None::<Range<usize>>; leaves.len()];
        let mut row = 0;
        let mut column = 0;
        let mut cell = None;
        for (event, range) in
            Parser::new_ext(source, crate::content::markdown::parse::OPTIONS).into_offset_iter()
        {
            match &event {
                Event::Start(Tag::Table(_)) | Event::Start(Tag::TableHead) => {
                    row = 0;
                    column = 0;
                }
                Event::Start(Tag::TableRow) => {
                    row += 1;
                    column = 0;
                }
                Event::Start(Tag::TableCell) => {
                    cell = Some(Part::Cell { row, column });
                }
                Event::End(TagEnd::TableCell) => {
                    cell = None;
                    column += 1;
                }
                _ => {}
            }
            let Some(index) = partitions
                .iter()
                .position(|part| part.start <= range.start && range.start < part.end)
            else {
                continue;
            };
            let Some(block) = doc.blocks.get(index) else {
                continue;
            };
            let leaf = &mut leaves[index];
            if matches!(
                event,
                Event::Start(
                    Tag::Paragraph
                        | Tag::Heading { .. }
                        | Tag::CodeBlock(_)
                        | Tag::Table(_)
                        | Tag::HtmlBlock
                ) | Event::Rule
            ) {
                extents[index] = Some(range.clone());
            }
            let part = match block.kind {
                BlockKind::Code { .. } => Part::Code,
                BlockKind::Image { .. } => Part::Caption,
                BlockKind::Table { .. } => match cell {
                    Some(part) => part,
                    None => continue,
                },
                _ => Part::Body,
            };
            let Some((_, span)) = leaf
                .parts
                .iter_mut()
                .find(|(candidate, _)| *candidate == part)
            else {
                continue;
            };
            match event {
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => {
                    let at = source[range.clone()]
                        .find('\n')
                        .map_or(range.end, |offset| range.start + offset + 1);
                    let line = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
                    let prefix = continuation(&source[line..range.start]);
                    let shortened = prefix.trim_end_matches([' ', '\t']);
                    let prefix = if !source[at..].starts_with(&prefix)
                        && !shortened.is_empty()
                        && source[at..].starts_with(shortened)
                    {
                        shortened.to_owned()
                    } else {
                        prefix
                    };
                    let at = if source[at..].starts_with(&prefix) {
                        at + prefix.len()
                    } else {
                        at
                    };
                    // Empty fenced blocks have no Text event. Their caret
                    // belongs after the opening line, not after the closing one.
                    span.syntax(at..at);
                    span.prefix = Some(prefix);
                }
                Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
                    span.push(&value, range, source, false)
                }
                Event::Code(value) => span.push(&value, range, source, true),
                Event::SoftBreak | Event::HardBreak => span.push("\n", range, source, false),
                Event::Start(
                    Tag::Emphasis
                    | Tag::Strong
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. },
                ) => span.syntax(range),
                Event::Start(Tag::TableCell) => {
                    let raw = &source[range.clone()];
                    let trim = raw.len() - raw.trim_start_matches([' ', '\t', '|']).len();
                    span.syntax(range.start + trim..range.start + trim);
                }
                _ => {}
            }
        }
        for (index, leaf) in leaves.iter_mut().enumerate() {
            let block = &doc.blocks[index];
            for (part, span) in &mut leaf.parts {
                if let Some(text) = block.text_at(*part)
                    && let Some(aligned) = span.align_live(&text.text, source)
                {
                    *span = aligned;
                }
            }
            let covered = leaf
                .parts
                .iter()
                .filter_map(|(_, span)| span.syntax.clone())
                .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
            if let Some(mut range) = extents[index].clone().or(covered) {
                range.start = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
                range.end = range.end.min(leaf.range.end);
                while range.end > range.start
                    && matches!(source.as_bytes()[range.end - 1], b'\r' | b'\n')
                {
                    range.end -= 1;
                }
                leaf.range = range;
            } else {
                leaf.range.end = leaf.range.start;
            }
        }
        Self { leaves }
    }

    pub fn offset(&self, cursor: Cursor) -> Option<usize> {
        let leaf = self.leaves.get(cursor.block)?;
        let span = &leaf.parts.iter().find(|(part, _)| *part == cursor.part)?.1;
        span.offset(cursor.offset)
            .or_else(|| (span.value.is_empty() && cursor.offset == 0).then_some(leaf.range.end))
    }

    pub fn cursor(&self, offset: usize, doc: &Doc) -> Cursor {
        self.leaves
            .iter()
            .enumerate()
            .flat_map(|(block, leaf)| {
                leaf.parts.iter().flat_map(move |(part, span)| {
                    span.characters
                        .iter()
                        .flat_map(move |entry| {
                            [
                                (
                                    entry.raw.start.abs_diff(offset),
                                    Cursor::new(block, *part, entry.text.start),
                                ),
                                (
                                    entry.raw.end.abs_diff(offset),
                                    Cursor::new(block, *part, entry.text.end),
                                ),
                            ]
                        })
                        .chain(span.characters.is_empty().then(|| {
                            (
                                span.offset(0).unwrap_or(leaf.range.end).abs_diff(offset),
                                Cursor::new(block, *part, 0),
                            )
                        }))
                })
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, cursor)| cursor)
            .unwrap_or_default()
            .clamp(doc)
    }

    pub fn patch(&self, source: &str, before: &Doc, after: &Doc) -> Option<Patch> {
        let first = before
            .blocks
            .iter()
            .zip(&after.blocks)
            .take_while(|(a, b)| a == b)
            .count();
        if first == before.blocks.len() && first == after.blocks.len() {
            return None;
        }
        let tail = before.blocks[first..]
            .iter()
            .rev()
            .zip(after.blocks[first..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let old_end = before.blocks.len() - tail;
        let new_end = after.blocks.len() - tail;
        if old_end == first + 1 && new_end == first + 1 {
            let old = &before.blocks[first];
            let new = &after.blocks[first];
            let changed: Vec<_> = old
                .parts()
                .into_iter()
                .filter(|part| old.text_at(*part) != new.text_at(*part))
                .collect();
            if changed.len() == 1 {
                let part = changed[0];
                let mut shape = old.clone();
                if let (Some(target), Some(value)) = (shape.text_at_mut(part), new.text_at(part)) {
                    *target = value.clone();
                }
                if shape == *new
                    && let Some(patch) =
                        self.part_patch(source, first, part, old.text_at(part)?, new.text_at(part)?)
                {
                    return Some(spelling::preserve(self, source, before, after, patch));
                }
            }
        }
        let mut start = self
            .leaves
            .get(first)
            .map_or(source.len(), |leaf| leaf.range.start);
        let end = if old_end == first {
            start
        } else {
            old_end
                .checked_sub(1)
                .and_then(|index| self.leaves.get(index))
                .map_or(start, |leaf| leaf.range.end)
        };
        let ending = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut text = String::new();
        let mut retained_alert = None;
        if old_end > first
            && let (Some(old), Some(new)) = (before.blocks.get(first), after.blocks.get(first))
        {
            for (event, range) in
                Parser::new_ext(source, crate::content::markdown::parse::OPTIONS).into_offset_iter()
            {
                let Event::Start(Tag::BlockQuote(Some(kind))) = event else {
                    continue;
                };
                let container = Container::Quote(Some(kind.into()));
                if range.start < start
                    && start < range.end
                    && old.containers.contains(&container)
                    && !new.containers.contains(&container)
                    && first
                        .checked_sub(1)
                        .and_then(|i| self.leaves.get(i))
                        .is_none_or(|leaf| leaf.range.end <= range.start)
                {
                    let header = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
                    if self
                        .leaves
                        .get(old_end)
                        .is_some_and(|leaf| leaf.range.start < range.end)
                    {
                        retained_alert = Some(source[header..start].trim_end_matches(['\r', '\n']));
                    }
                    start = header;
                    break;
                }
            }
            // A newly nested block must join its parent's quote scope. The
            // old unquoted separator closes that scope before the new prefix
            // is read, turning a list continuation into an indented code block.
            if first > 0
                && new.indent > old.indent
                && new.containers != old.containers
                && new
                    .containers
                    .iter()
                    .any(|c| matches!(c, Container::Quote(_)))
                && let Some(previous) = self.leaves.get(first - 1)
                && source[previous.range.end..start]
                    .chars()
                    .all(char::is_whitespace)
            {
                start = previous.range.end;
                text.push_str(ending);
                text.push_str(&container_prefix(new, false));
                text.push_str(ending);
            }
        }
        if old_end == first && first > 0 {
            let trailing = source[..start]
                .chars()
                .rev()
                .take_while(|ch| matches!(ch, '\r' | '\n'))
                .filter(|ch| *ch == '\n')
                .count();
            let previous = &after.blocks[first - 1];
            let tight = after.blocks.get(first).is_some_and(|block| {
                marker(previous) && marker(block) && previous.containers == block.containers
            });
            for _ in trailing..if tight { 1 } else { 2 } {
                text.push_str(ending);
            }
        }
        for (position, block) in after.blocks[first..new_end].iter().enumerate() {
            if position > 0 {
                text.push_str(ending);
                let previous = &after.blocks[first + position - 1];
                if !(marker(previous) && marker(block) && previous.containers == block.containers) {
                    text.push_str(&container_prefix(block, false));
                    text.push_str(ending);
                }
            }
            text.push_str(&block_source(block, ending));
        }
        if old_end == first && first < before.blocks.len() {
            text.push_str(ending);
            text.push_str(ending);
        }
        if let Some(header) = retained_alert {
            text.push_str(ending);
            text.push_str(ending);
            text.push_str(header);
        }
        let parser = Parser::new_ext(source, crate::content::markdown::parse::OPTIONS);
        for (_, definition) in parser.reference_definitions().iter() {
            let range = definition.span.clone();
            if start <= range.start && range.end <= end {
                text.push_str(ending);
                text.push_str(ending);
                text.push_str(&source[range]);
            }
        }
        Some(spelling::preserve(
            self,
            source,
            before,
            after,
            Patch {
                range: start..end,
                text,
            },
        ))
    }

    fn part_patch(
        &self,
        source: &str,
        block: usize,
        part: Part,
        before: &Text,
        after: &Text,
    ) -> Option<Patch> {
        let span = &self
            .leaves
            .get(block)?
            .parts
            .iter()
            .find(|(candidate, _)| *candidate == part)?
            .1;
        let (range, added) = change(&before.text, &after.text);
        let mut expected = before.clone();
        expected.remove(range.clone());
        expected.insert(range.start, added);
        let literal = matches!(part, Part::Code | Part::Caption);
        if expected == *after
            && let Some(raw) = span.raw_range(range)
        {
            let text = if literal {
                added.to_owned()
            } else {
                super::serialize::text(&Text::plain(added))
            };
            let visible = if literal {
                after.clone()
            } else {
                crate::content::markdown::parse::normalize(&after.text, &after.marks)
            };
            let boundary =
                (part == Part::Code && before.text.is_empty() && raw.is_empty()).then(|| {
                    if source[..raw.start].ends_with('\n')
                        || source[..raw.start]
                            .rsplit_once('\n')
                            .is_some_and(|(_, prefix)| {
                                prefix.chars().all(|ch| ch.is_whitespace() || ch == '>')
                            })
                    {
                        format!("{text}\n")
                    } else {
                        format!("\n{text}")
                    }
                });
            for text in std::iter::once(text).chain(boundary) {
                let text = multiline(source, &raw, &text, span.prefix.as_deref());
                let mut candidate = source.to_owned();
                candidate.replace_range(raw.clone(), &text);
                if parse(&candidate)
                    .blocks
                    .get(block)
                    .and_then(|block| block.text_at(part))
                    .is_some_and(|value| {
                        value.text.trim_end() == visible.text.trim_end()
                            && value.marks == visible.marks
                    })
                {
                    return Some(Patch { range: raw, text });
                }
            }
        }
        if matches!(part, Part::Code | Part::Caption) {
            return None;
        }
        let raw = span.syntax.clone()?;
        let text = multiline(source, &raw, &super::serialize::text(after), None);
        Some(Patch { range: raw, text })
    }
}

fn marker(block: &Block) -> bool {
    matches!(
        block.kind,
        BlockKind::Bullet(_) | BlockKind::Ordered { .. } | BlockKind::Task { .. }
    )
}

fn container_prefix(block: &Block, first: bool) -> String {
    let mut output = String::new();
    let last_list = marker(block)
        .then(|| {
            block
                .containers
                .iter()
                .rposition(|container| matches!(container, Container::List))
        })
        .flatten();
    for (index, container) in block.containers.iter().enumerate() {
        match container {
            Container::Quote(_) => output.push_str("> "),
            Container::List if Some(index) == last_list && first => {}
            Container::List => output.push_str("    "),
        }
    }
    output
}

fn block_source(block: &Block, ending: &str) -> String {
    let mut leaf = block.clone();
    if !leaf.containers.is_empty()
        && let BlockKind::Quote { text, .. } = &leaf.kind
    {
        leaf.kind = BlockKind::Paragraph(text.clone());
    }
    let value = super::serialize::block(&leaf);
    let first = container_prefix(block, true);
    let rest = container_prefix(block, false);
    value
        .split('\n')
        .enumerate()
        .map(|(index, line)| format!("{}{line}", if index == 0 { &first } else { &rest }))
        .collect::<Vec<_>>()
        .join(ending)
}

fn multiline(source: &str, range: &Range<usize>, text: &str, prefix: Option<&str>) -> String {
    let ending = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let start = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
    let current = &source[start..range.start];
    let detected = continuation(current);
    let missing = prefix.filter(|_| current.is_empty()).unwrap_or_default();
    let prefix = prefix.unwrap_or(&detected);
    format!(
        "{missing}{}",
        text.replace("\r\n", "\n")
            .replace('\n', &format!("{ending}{prefix}"))
    )
}

pub(super) fn change<'a>(before: &str, after: &'a str) -> (Range<usize>, &'a str) {
    let prefix: usize = before
        .chars()
        .zip(after.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum();
    let suffix: usize = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum();
    (
        prefix..before.len() - suffix,
        &after[prefix..after.len() - suffix],
    )
}

fn continuation(prefix: &str) -> String {
    let bytes = prefix.as_bytes();
    let mut output = String::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b' ' | b'\t' | b'>' => {
                output.push(bytes[at] as char);
                at += 1;
            }
            b'-' | b'+' | b'*' if bytes.get(at + 1).is_some_and(u8::is_ascii_whitespace) => {
                output.push(' ');
                at += 1;
            }
            b'0'..=b'9' => {
                let start = at;
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
                if matches!(bytes.get(at), Some(b'.' | b')'))
                    && bytes.get(at + 1).is_some_and(u8::is_ascii_whitespace)
                {
                    output.push_str(&" ".repeat(at + 1 - start));
                    at += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    output
}

#[cfg(test)]
mod tests;
