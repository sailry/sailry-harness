use std::{borrow::Cow, ops::Range};

pub(super) struct View<'a> {
    raw: &'a str,
    normalized: Cow<'a, str>,
    removed: Vec<usize>,
}

impl<'a> View<'a> {
    pub(super) fn new(raw: &'a str) -> Self {
        let mut removed = Vec::new();
        for (position, _) in raw.match_indices("\r\n") {
            removed.push(position - removed.len());
        }
        Self {
            raw,
            normalized: normalize(raw),
            removed,
        }
    }

    fn original(&self, position: usize) -> usize {
        position + self.removed.partition_point(|removed| *removed < position)
    }
}

pub(super) struct Pattern<'a> {
    text: Cow<'a, str>,
    raw: bool,
}

impl<'a> Pattern<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        // A lone carriage return is a literal edit, rather than a line terminator.
        let raw = text.bytes().enumerate().any(|(position, byte)| {
            byte == b'\r' && text.as_bytes().get(position + 1) != Some(&b'\n')
        });
        Self {
            text: if raw {
                Cow::Borrowed(text)
            } else {
                normalize(text)
            },
            raw,
        }
    }

    fn input<'b>(&self, view: &'b View<'_>) -> &'b str {
        if self.raw { view.raw } else { &view.normalized }
    }

    pub(super) fn find(&self, view: &View<'_>, position: usize) -> Option<usize> {
        self.input(view)[position..]
            .find(self.text.as_ref())
            .map(|found| position + found)
    }

    pub(super) fn following(&self, view: &View<'_>, position: usize) -> usize {
        position
            + self.input(view)[position..]
                .chars()
                .next()
                .unwrap()
                .len_utf8()
    }

    pub(super) fn range(&self, view: &View<'_>, position: usize) -> Range<usize> {
        let end = position + self.text.len();
        if self.raw {
            position..end
        } else {
            view.original(position)..view.original(end)
        }
    }

    pub(super) fn replacement<'b>(
        &self,
        before: &str,
        range: Range<usize>,
        text: &'b str,
    ) -> Cow<'b, str> {
        if self.raw {
            Cow::Borrowed(text)
        } else {
            Cow::Owned(replacement(before, range, text))
        }
    }
}

fn normalize(text: &str) -> Cow<'_, str> {
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

fn replacement(before: &str, range: Range<usize>, text: &str) -> String {
    let matched = &before[range.clone()];
    let endings: Vec<_> = matched
        .match_indices('\n')
        .map(|(position, _)| {
            if position > 0 && matched.as_bytes()[position - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            }
        })
        .collect();
    let nearby = endings.last().copied().unwrap_or_else(|| {
        let position = before[range.start..]
            .find('\n')
            .map(|position| range.start + position)
            .or_else(|| before[..range.start].rfind('\n'));
        if position.is_some_and(|position| position > 0 && before.as_bytes()[position - 1] == b'\r')
        {
            "\r\n"
        } else {
            "\n"
        }
    });
    let normalized = normalize(text);
    let mut after = String::with_capacity(text.len());
    let mut offset = 0;
    for (index, (position, _)) in normalized.match_indices('\n').enumerate() {
        after.push_str(&normalized[offset..position]);
        after.push_str(endings.get(index).copied().unwrap_or(nearby));
        offset = position + 1;
    }
    after.push_str(&normalized[offset..]);
    after
}
