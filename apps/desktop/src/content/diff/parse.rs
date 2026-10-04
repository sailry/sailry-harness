#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Context,
    Added,
    Removed,
    Hunk,
    Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Line<'a> {
    pub kind: Kind,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: &'a str,
}

/// Only Git's binary markers outside a textual hunk denote an unavailable diff.
pub(crate) fn is_binary(lines: &[Line<'_>]) -> bool {
    !lines
        .iter()
        .any(|line| line.old.is_some() || line.new.is_some())
        && lines.iter().any(|line| {
            line.kind == Kind::Meta
                && (line.text == "GIT binary patch"
                    || (line.text.starts_with("Binary files ") && line.text.ends_with(" differ")))
        })
}

/// Preserve both source coordinates across hunks, including empty ranges.
pub(crate) fn unified(text: &str) -> Vec<Line<'_>> {
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    text.lines()
        .map(|line| {
            if line.starts_with("diff --git ") {
                in_hunk = false;
            }
            let kind = if line.starts_with("@@ ") {
                in_hunk = false;
                let mut fields = line.split_whitespace().skip(1);
                let start = |field: Option<&str>, prefix: char| {
                    field?
                        .strip_prefix(prefix)?
                        .split(',')
                        .next()?
                        .parse::<usize>()
                        .ok()
                };
                if let (Some(before), Some(after)) =
                    (start(fields.next(), '-'), start(fields.next(), '+'))
                {
                    (old, new) = (before, after);
                    in_hunk = true;
                }
                Kind::Hunk
            } else if !in_hunk {
                Kind::Meta
            } else if line.starts_with('+') {
                Kind::Added
            } else if line.starts_with('-') {
                Kind::Removed
            } else if line.starts_with(' ') {
                Kind::Context
            } else {
                Kind::Meta
            };
            let (before, after) = match kind {
                Kind::Added => {
                    let value = new;
                    new += 1;
                    (None, Some(value))
                }
                Kind::Removed => {
                    let value = old;
                    old += 1;
                    (Some(value), None)
                }
                Kind::Context => {
                    let pair = (Some(old), Some(new));
                    old += 1;
                    new += 1;
                    pair
                }
                _ => (None, None),
            };
            Line {
                kind,
                old: before,
                new: after,
                text: if before.is_some() || after.is_some() {
                    &line[1..]
                } else {
                    line
                },
            }
        })
        .collect()
}

pub(crate) fn additions(text: &str) -> Vec<Line<'_>> {
    text.lines()
        .enumerate()
        .map(|(index, text)| Line {
            kind: Kind::Added,
            old: None,
            new: Some(index + 1),
            text,
        })
        .collect()
}
