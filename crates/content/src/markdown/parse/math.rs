//! Recognize LaTeX delimiters while keeping CommonMark's source byte offsets.
use pulldown_cmark::{CowStr, Event, OffsetIter, Options, Parser, Tag, TagEnd};
use std::ops::Range;

pub(crate) struct Events<'a> {
    original: Option<OffsetIter<'a>>,
    formulas: std::vec::IntoIter<(Event<'a>, Range<usize>)>,
}

impl<'a> Events<'a> {
    fn original(events: OffsetIter<'a>) -> Self {
        Self {
            original: Some(events),
            formulas: Vec::new().into_iter(),
        }
    }
}

impl<'a> Iterator for Events<'a> {
    type Item = (Event<'a>, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.original {
            Some(original) => original.next(),
            None => self.formulas.next(),
        }
    }
}

struct Formula {
    range: Range<usize>,
    display: bool,
    containers: bool,
    delimiter_bytes: usize,
}

pub(crate) fn events(source: &str) -> Events<'_> {
    let events = formulas(source);
    if !source.contains('$') {
        return events;
    }
    let mut result = Vec::new();
    let mut links = 0;
    for (event, range) in events {
        match &event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => links += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => links -= 1,
            Event::InlineMath(_) if links == 0 && prices(source, &range) => {
                // Reparse only this false formula, without math. The original
                // delimiters and inline Markdown retain their source offsets.
                let options = super::OPTIONS.difference(Options::ENABLE_MATH);
                for (literal, local) in
                    Parser::new_ext(&source[range.clone()], options).into_offset_iter()
                {
                    if !matches!(
                        literal,
                        Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph)
                    ) {
                        result.push((literal, range.start + local.start..range.start + local.end));
                    }
                }
                continue;
            }
            _ => {}
        }
        result.push((event, range));
    }
    Events {
        original: None,
        formulas: result.into_iter(),
    }
}

fn prices(source: &str, range: &Range<usize>) -> bool {
    let raw = &source[range.clone()];
    let Some(body) = raw
        .strip_prefix('$')
        .and_then(|value| value.strip_suffix('$'))
    else {
        return false;
    };
    if raw.starts_with("$$")
        || !body.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || !source
            .as_bytes()
            .get(range.end)
            .is_some_and(u8::is_ascii_digit)
        || body.contains([
            '\n', '\r', '\\', '{', '}', '_', '^', '=', '+', '*', '<', '>', '-',
        ])
    {
        return false;
    }
    let amount = body
        .bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .count();
    let rest = &body[amount..];
    if matches!(rest.trim(), "," | "、" | "，" | ";" | "；") {
        return true;
    }
    if !rest.chars().any(char::is_whitespace) && !rest.starts_with('/') {
        return false;
    }
    let mut letters = 0;
    rest.chars().any(|character| {
        letters = if character.is_alphabetic() {
            letters + 1
        } else {
            0
        };
        letters >= 2
    })
}

fn formulas(source: &str) -> Events<'_> {
    let original = || Parser::new_ext(source, super::OPTIONS).into_offset_iter();
    if !source.contains(r"\(")
        && !source.contains(r"\[")
        && !source.contains(" $")
        && !source.contains("\t$")
    {
        return Events::original(original());
    }
    let parser = Parser::new_ext(source, super::OPTIONS);
    let mut protected: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .collect();
    let mut barriers = protected.clone();
    let mut containers = Vec::new();
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock) | Event::Html(_) => {
                barriers.push(range.clone());
                protected.push(range);
            }
            Event::Code(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Start(Tag::Link { .. } | Tag::Image { .. }) => {
                protected.push(range);
            }
            Event::Start(Tag::BlockQuote(_) | Tag::Item) => containers.push(range),
            _ => {}
        }
    }
    protected.sort_by_key(|range| range.start);
    let protected = merge(protected);
    let is_protected = |at| {
        protected
            .partition_point(|range| range.start <= at)
            .checked_sub(1)
            .is_some_and(|index| protected[index].contains(&at))
    };
    let tokens = delimiters(source);
    let inline_closes: Vec<_> = tokens
        .iter()
        .filter_map(|&(at, token)| (token == b')' && !is_protected(at)).then_some(at))
        .collect();
    let display_closes: Vec<_> = tokens
        .iter()
        .filter_map(|&(at, token)| (token == b']' && !is_protected(at)).then_some(at))
        .collect();
    let dollar_closes: Vec<_> = tokens
        .iter()
        .filter_map(|&(at, token)| (token == b'$' && !is_protected(at)).then_some(at))
        .collect();
    let mut formulas = Vec::new();
    let mut after = 0;
    for &(start, delimiter) in &tokens {
        if start < after || !matches!(delimiter, b'(' | b'[' | b'$') || is_protected(start) {
            continue;
        }
        let closes = match delimiter {
            b'(' => &inline_closes,
            b'[' => &display_closes,
            _ => &dollar_closes,
        };
        let Some(&end) = closes.get(closes.partition_point(|&at| at <= start)) else {
            continue;
        };
        let delimiter_bytes = if delimiter == b'$' { 1 } else { 2 };
        let range = start..end + delimiter_bytes;
        let body = &source[start + delimiter_bytes..end];
        if body.trim().is_empty()
            || (delimiter == b'$'
                && (!numeric_body(body)
                    || protected
                        .iter()
                        .any(|protected| start < protected.end && protected.start < range.end)))
            || barriers
                .iter()
                .any(|barrier| start < barrier.end && barrier.start < range.end)
        {
            continue;
        }
        let contained = containers
            .iter()
            .any(|container| container.start <= start && range.end <= container.end);
        after = range.end;
        formulas.push(Formula {
            range,
            display: delimiter == b'[',
            containers: contained,
            delimiter_bytes,
        });
    }
    if formulas.is_empty() {
        return Events::original(original());
    }
    // Preserve each delimiter's width. Mask the payload, rather than
    // substituting its TeX into dollar math: embedded dollars and Markdown
    // punctuation must never change its interpretation or its source offsets.
    let mut masked = source.as_bytes().to_vec();
    for formula in &formulas {
        let range = &formula.range;
        let width = formula.delimiter_bytes;
        masked[range.start..range.start + width].fill(b'$');
        masked[range.end - width..range.end].fill(b'$');
        let mut prefix = false;
        for byte in &mut masked[range.start + width..range.end - width] {
            if matches!(*byte, b'\r' | b'\n') {
                prefix = formula.containers;
            } else if !(prefix && matches!(*byte, b' ' | b'\t' | b'>')) {
                *byte = b'x';
                prefix = false;
            }
        }
    }
    let masked = String::from_utf8(masked).expect("Formula masking preserves UTF-8");
    let mut result = Vec::new();
    let mut recognized = 0;
    for (event, range) in Parser::new_ext(&masked, super::OPTIONS).into_offset_iter() {
        if let Event::DisplayMath(value) | Event::InlineMath(value) = &event
            && let Some(formula) = formulas.iter().find(|formula| formula.range == range)
        {
            let body = restore(source, formula, value);
            let event = if formula.display {
                Event::DisplayMath(body)
            } else {
                Event::InlineMath(body)
            };
            result.push((event, range));
            recognized += 1;
        } else {
            result.push((event.into_static(), range));
        }
    }
    if recognized == formulas.len() {
        Events {
            original: None,
            formulas: result.into_iter(),
        }
    } else {
        // Incomplete Markdown containers must retain authored text, never the
        // parser's private placeholder payload.
        Events::original(original())
    }
}

fn numeric_body(body: &str) -> bool {
    // Preserve spaced numeric algebra without treating currency prose as math.
    if !body.ends_with([' ', '\t']) || body.contains(['\n', '\r']) {
        return false;
    }
    let mut words = body.split_ascii_whitespace();
    let (Some(coefficient), Some(variable)) = (words.next(), words.next()) else {
        return false;
    };
    coefficient
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_digit)
        && coefficient
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && variable.len() == 1
        && variable.as_bytes()[0].is_ascii_alphabetic()
        && words.next().is_none()
}

fn merge(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut result: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = result.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            result.push(range);
        }
    }
    result
}

fn delimiters(source: &str) -> Vec<(usize, u8)> {
    let mut result = Vec::new();
    let mut slashes = 0;
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'$'
            && slashes % 2 == 0
            && index
                .checked_sub(1)
                .and_then(|at| source.as_bytes().get(at))
                != Some(&b'$')
            && source.as_bytes().get(index + 1) != Some(&b'$')
        {
            result.push((index, b'$'));
        }
        if byte == b'\\' {
            if slashes % 2 == 0
                && let Some(&delimiter) = source.as_bytes().get(index + 1)
                && matches!(delimiter, b'(' | b')' | b'[' | b']')
            {
                result.push((index, delimiter));
            }
            slashes += 1;
        } else {
            slashes = 0;
        }
    }
    result
}

fn restore<'a>(source: &'a str, formula: &Formula, value: &str) -> CowStr<'a> {
    let width = formula.delimiter_bytes;
    let body = &source[formula.range.start + width..formula.range.end - width];
    if body.len() == value.len() {
        return body.into();
    }
    // CommonMark removes continuation prefixes inside quotes and lists.
    // Masked payload bytes are otherwise unchanged in length, so each line's
    // removed prefix maps directly back to the original TeX.
    if body.bytes().filter(|&byte| byte == b'\n').count()
        != value.bytes().filter(|&byte| byte == b'\n').count()
    {
        return body.into();
    }
    let mut restored = String::new();
    for (raw, parsed) in body.split_inclusive('\n').zip(value.split_inclusive('\n')) {
        let Some(line) = raw
            .len()
            .checked_sub(parsed.len())
            .and_then(|removed| raw.get(removed..))
        else {
            return body.into();
        };
        restored.push_str(line);
    }
    restored.into()
}

#[cfg(test)]
mod tests;
