//! Inline thinking filter derived from Goose 14741e9 (Apache-2.0).
const BUFFER_LIMIT: usize = 8 * 1024;

#[derive(Default)]
pub(super) struct Filter {
    buffer: String,
    depth: usize,
}

#[derive(Default)]
pub(super) struct Output {
    pub text: String,
    pub thinking: String,
}

enum Tag {
    Open,
    Close,
    Empty,
}

impl Filter {
    pub fn push(&mut self, text: &str) -> Output {
        self.buffer.push_str(text);
        let mut output = Output::default();
        let mut cursor = 0;
        let mut consumed = 0;
        while let Some(offset) = self.buffer[cursor..].find('<') {
            let start = cursor + offset;
            let suffix = &self.buffer[start..];
            if let Some((tag, length)) = parse(suffix) {
                let end = start + length;
                if length <= BUFFER_LIMIT && (self.depth > 0 || !matches!(tag, Tag::Close)) {
                    self.append(&mut output, &self.buffer[consumed..start]);
                    match tag {
                        Tag::Open => self.depth += 1,
                        Tag::Close => self.depth = self.depth.saturating_sub(1),
                        Tag::Empty => {}
                    }
                    consumed = end;
                    cursor = end;
                    continue;
                }
            } else if partial(suffix) && suffix.len() <= BUFFER_LIMIT {
                self.append(&mut output, &self.buffer[consumed..start]);
                self.buffer.drain(..start);
                return output;
            }
            cursor = start + 1;
        }
        self.append(&mut output, &self.buffer[consumed..]);
        self.buffer.clear();
        if self.buffer.capacity() > BUFFER_LIMIT {
            self.buffer = String::new();
        }
        output
    }

    pub fn finish(mut self) -> Output {
        let mut output = self.push("");
        self.append(&mut output, &self.buffer);
        output
    }

    fn append(&self, output: &mut Output, text: &str) {
        if self.depth > 0 {
            output.thinking.push_str(text);
        } else {
            output.text.push_str(text);
        }
    }
}

fn parse(text: &str) -> Option<(Tag, usize)> {
    let bytes = text.as_bytes();
    let close = bytes.get(1) == Some(&b'/');
    let start = if close { 2 } else { 1 };
    let mut cursor = start;
    while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
        cursor += 1;
    }
    let name = text.get(start..cursor)?;
    if !name.eq_ignore_ascii_case("think") && !name.eq_ignore_ascii_case("thinking") {
        return None;
    }
    if close {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        return (bytes.get(cursor) == Some(&b'>')).then_some((Tag::Close, cursor + 1));
    }
    if !bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'>' | b'/') || byte.is_ascii_whitespace())
    {
        return None;
    }
    let mut quote = None;
    let mut last = None;
    while let Some(&byte) = bytes.get(cursor) {
        match quote {
            Some(delimiter) if byte == delimiter => quote = None,
            Some(_) => {}
            None if matches!(byte, b'\'' | b'"') => {
                quote = Some(byte);
                last = Some(byte);
            }
            None if byte == b'>' => {
                return Some((
                    if last == Some(b'/') {
                        Tag::Empty
                    } else {
                        Tag::Open
                    },
                    cursor + 1,
                ));
            }
            None if !byte.is_ascii_whitespace() => last = Some(byte),
            None => {}
        }
        cursor += 1;
    }
    None
}

fn partial(text: &str) -> bool {
    let close = text.starts_with("</");
    let suffix = if close { &text[2..] } else { &text[1..] };
    let end = suffix
        .bytes()
        .position(|byte| !byte.is_ascii_alphabetic())
        .unwrap_or(suffix.len());
    let name = &suffix[..end];
    let lower = name.to_ascii_lowercase();
    if end == suffix.len() {
        return "think".starts_with(&lower) || "thinking".starts_with(&lower);
    }
    if !matches!(lower.as_str(), "think" | "thinking") {
        return false;
    }
    let rest = &suffix[end..];
    if close {
        return rest.bytes().all(|byte| byte.is_ascii_whitespace());
    }
    if !rest
        .as_bytes()
        .first()
        .is_some_and(|byte| *byte == b'/' || byte.is_ascii_whitespace())
    {
        return false;
    }
    let mut quote = None;
    for byte in rest.bytes() {
        match quote {
            Some(delimiter) if byte == delimiter => quote = None,
            Some(_) => {}
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'>' => return false,
            None => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_across_every_boundary() {
        for input in [
            "<think>hidden</think>visible",
            "<THINKING data='a>b'>hidden</thinking>visible",
            "<think>hid<think/>den</think>visible",
        ] {
            for boundary in input.char_indices().map(|(index, _)| index) {
                let mut filter = Filter::default();
                let first = filter.push(&input[..boundary]);
                let second = filter.push(&input[boundary..]);
                let last = filter.finish();
                assert_eq!(
                    format!("{}{}{}", first.text, second.text, last.text),
                    "visible"
                );
                assert_eq!(
                    format!("{}{}{}", first.thinking, second.thinking, last.thinking),
                    "hidden"
                );
            }
        }
    }

    #[test]
    fn preserves_plain_and_unfinished_text() {
        for input in [
            "你好 <thinking-mode>x",
            "</think>shown",
            "text<thin",
            "<think/>shown",
        ] {
            let mut filter = Filter::default();
            let first = filter.push(input);
            let last = filter.finish();
            let expected = input.strip_prefix("<think/>").unwrap_or(input);
            assert_eq!(format!("{}{}", first.text, last.text), expected);
            assert!(first.thinking.is_empty());
            assert!(last.thinking.is_empty());
        }
    }

    #[test]
    fn retains_unclosed_reasoning() {
        let mut filter = Filter::default();
        let first = filter.push("<think>hidden</thi");
        let last = filter.finish();
        assert!(first.text.is_empty());
        assert!(last.text.is_empty());
        assert_eq!(
            format!("{}{}", first.thinking, last.thinking),
            "hidden</thi"
        );
    }
}
