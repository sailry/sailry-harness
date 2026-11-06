//! Update hunks adapted from openai/codex 451787127c0e70247034c33e358536ffd2b89cf8
//! codex-rs/apply-patch/src/file_update.rs, text_file.rs and seek_sequence.rs
//! (Apache-2.0). Line endings, chunk order, `@@` anchors, end-of-file anchors and kept
//! context lines follow Codex. Codex's trailing-whitespace, whitespace and Unicode
//! punctuation fallbacks are omitted: context and removed lines match exactly, and a
//! chunk whose lines also match before the next chunk (or the end of the file) is
//! ambiguous. The result is expressed as exact edits so the change-set engine
//! validates and writes it like any other edit.
use super::Chunk;
use crate::files::edit::{self, text};
use sailry_protocol::{Fault, FileEdit};

/// Replaces `old` lines from `start` with new line texts.
type Replacement = (usize, usize, Vec<String>);

/// Original bytes scanned while making edit anchors unique, before the whole file is used.
const SCAN_BYTES: usize = 512 * 1024 * 1024;
/// Quoted patch lines kept in a rejection reason.
const QUOTE_BYTES: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Lf,
    CrLf,
    Cr,
}

impl Ending {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

struct Line<'a> {
    text: &'a str,
    ending: Option<Ending>,
    /// Byte offset in the original text.
    start: usize,
}

impl Line<'_> {
    fn end(&self) -> usize {
        self.start + self.text.len() + self.ending.map_or(0, |ending| ending.as_str().len())
    }
}

/// A file split into lines that keep their endings. The first ending is used for
/// inserted lines; a file without one uses LF.
struct Source<'a> {
    lines: Vec<Line<'a>>,
    preferred: Ending,
}

impl<'a> Source<'a> {
    fn parse(contents: &'a str) -> Self {
        let bytes = contents.as_bytes();
        let mut lines = Vec::new();
        let mut preferred = None;
        let mut start = 0;
        let mut cursor = 0;
        while cursor < bytes.len() {
            let (ending, length) = match bytes[cursor] {
                b'\r' if bytes.get(cursor + 1) == Some(&b'\n') => (Ending::CrLf, 2),
                b'\r' => (Ending::Cr, 1),
                b'\n' => (Ending::Lf, 1),
                _ => {
                    cursor += 1;
                    continue;
                }
            };
            preferred.get_or_insert(ending);
            lines.push(Line {
                text: &contents[start..cursor],
                ending: Some(ending),
                start,
            });
            cursor += length;
            start = cursor;
        }
        if start < contents.len() {
            lines.push(Line {
                text: &contents[start..],
                ending: None,
                start,
            });
        }
        Self {
            lines,
            preferred: preferred.unwrap_or(Ending::Lf),
        }
    }

    /// Unchanged lines keep their endings and inserted lines use the preferred one.
    /// Every resulting line is terminated, as Codex updates always have been.
    fn render(&self, replacements: &[Replacement]) -> Result<String, Fault> {
        let mut output = String::new();
        let mut index = 0;
        for (start, length, new) in replacements {
            let unchanged = self
                .lines
                .get(index..*start)
                .ok_or_else(|| Fault::invalid("patch hunks overlap"))?;
            for line in unchanged {
                output.push_str(line.text);
                output.push_str(line.ending.unwrap_or(self.preferred).as_str());
            }
            for text in new {
                output.push_str(text);
                output.push_str(self.preferred.as_str());
            }
            index = start + length;
        }
        for line in self.lines.get(index..).unwrap_or_default() {
            output.push_str(line.text);
            output.push_str(line.ending.unwrap_or(self.preferred).as_str());
        }
        Ok(output)
    }
}

/// An update matched against one file revision.
pub(super) struct Update<'a> {
    source: Source<'a>,
    replacements: Vec<Replacement>,
    pub(super) after: String,
}

pub(super) fn apply<'a>(before: &'a str, chunks: &[Chunk]) -> Result<Update<'a>, Fault> {
    let source = Source::parse(before);
    let replacements = replacements(&source.lines, chunks)?;
    let after = source.render(&replacements)?;
    Ok(Update {
        source,
        replacements,
        after,
    })
}

/// A located chunk whose lines must not match again before the next chunk.
struct Located<'a> {
    hunk: usize,
    from: usize,
    at: usize,
    pattern: &'a [String],
}

fn replacements(lines: &[Line<'_>], chunks: &[Chunk]) -> Result<Vec<Replacement>, Fault> {
    let mut replacements = Vec::new();
    let mut located = Vec::new();
    let mut cursor = 0;
    for (index, chunk) in chunks.iter().enumerate() {
        let hunk = index + 1;
        if let Some(context) = &chunk.context {
            let at =
                seek(lines, std::slice::from_ref(context), cursor, false).ok_or_else(|| {
                    Fault::invalid(format!(
                        "hunk {hunk}: @@ context line was not found: {context}"
                    ))
                })?;
            cursor = at + 1;
        }
        if chunk.old.is_empty() {
            replacements.push((lines.len(), 0, chunk.new.clone()));
            continue;
        }
        // A trailing empty line may stand for the final newline, which is part of
        // the previous line's ending rather than a line of its own.
        let mut pattern = chunk.old.as_slice();
        let mut new = chunk.new.as_slice();
        let mut found = seek(lines, pattern, cursor, chunk.end_of_file);
        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            pattern = &pattern[..pattern.len() - 1];
            if new.last().is_some_and(String::is_empty) {
                new = &new[..new.len() - 1];
            }
            found = seek(lines, pattern, cursor, chunk.end_of_file);
        }
        let at = found.ok_or_else(|| {
            Fault::invalid(format!(
                "hunk {hunk}: expected lines were not found{}:\n{}",
                if chunk.end_of_file {
                    " at the end of the file"
                } else {
                    ""
                },
                quote(&chunk.old)
            ))
        })?;
        if !chunk.end_of_file && !pattern.is_empty() {
            located.push(Located {
                hunk,
                from: cursor,
                at,
                pattern,
            });
        }
        // Context lines stay in place so their exact text and endings survive.
        let mut old_start = 0;
        let mut new_start = 0;
        for &(old_context, new_context) in &chunk.kept {
            if old_context >= pattern.len() || new_context >= new.len() {
                break;
            }
            if old_start != old_context || new_start != new_context {
                replacements.push((
                    at + old_start,
                    old_context - old_start,
                    new[new_start..new_context].to_vec(),
                ));
            }
            old_start = old_context + 1;
            new_start = new_context + 1;
        }
        if old_start != pattern.len() || new_start != new.len() {
            replacements.push((
                at + old_start,
                pattern.len() - old_start,
                new[new_start..].to_vec(),
            ));
        }
        cursor = at + pattern.len();
    }
    for (position, chunk) in located.iter().enumerate() {
        let bound = located
            .get(position + 1)
            .map_or(lines.len(), |next| next.at);
        let count = (chunk.from..bound)
            .filter(|&other| matches(lines, chunk.pattern, other))
            .count();
        if count > 1 {
            return Err(Fault::invalid(format!(
                "hunk {}: expected lines match {count} locations; add context lines or an @@ line to identify one:\n{}",
                chunk.hunk,
                quote(chunk.pattern)
            )));
        }
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    Ok(replacements)
}

fn matches(lines: &[Line<'_>], pattern: &[String], at: usize) -> bool {
    lines.get(at..at + pattern.len()).is_some_and(|window| {
        window
            .iter()
            .zip(pattern)
            .all(|(line, text)| line.text == text)
    })
}

/// First exact match at or after `start`; an end-of-file chunk matches only at the end.
fn seek(lines: &[Line<'_>], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start);
    }
    let last = lines.len().checked_sub(pattern.len())?;
    let start = if eof { last.max(start) } else { start };
    (start..=last).find(|&at| matches(lines, pattern, at))
}

fn quote(lines: &[String]) -> String {
    let text = lines.join("\n");
    if text.len() <= QUOTE_BYTES {
        return text;
    }
    let mut end = QUOTE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[{} more bytes]", &text[..end], text.len() - end)
}

/// Replacement text for whole original lines `first..last`; empty ranges insert.
struct Region {
    first: usize,
    last: usize,
    text: String,
}

impl Update<'_> {
    /// Exact edits of `before` that the edit engine turns into [`Update::after`].
    /// Unchanged neighbors are added until each old text occurs once in the file.
    /// `None` means edits cannot reproduce the result, such as for an empty file or
    /// mixed line endings.
    pub(super) fn edits(&self, before: &str, revision: &str) -> Option<Vec<FileEdit>> {
        let lines = &self.source.lines;
        let count = lines.len();
        if count == 0 {
            return None;
        }
        let ending = self.source.preferred.as_str();
        let mut regions: Vec<Region> = self
            .replacements
            .iter()
            .map(|(start, length, new)| Region {
                first: *start,
                last: start + length,
                text: new.iter().map(|line| format!("{line}{ending}")).collect(),
            })
            .collect();
        // An unchanged unterminated final line still gains an ending.
        if let Some(line) = lines.last().filter(|line| line.ending.is_none())
            && !regions
                .iter()
                .any(|region| region.first < count && count <= region.last)
        {
            regions.push(Region {
                first: count - 1,
                last: count,
                text: format!("{}{ending}", line.text),
            });
        }
        regions.sort_by_key(|region| region.first);
        let mut merged: Vec<Region> = Vec::with_capacity(regions.len());
        for region in regions {
            match merged.last_mut() {
                Some(previous) if region.first <= previous.last => {
                    previous.last = previous.last.max(region.last);
                    previous.text.push_str(&region.text);
                }
                _ => merged.push(region),
            }
        }
        let view = text::View::new(before);
        let mut checks = (SCAN_BYTES / before.len().max(1)).max(8);
        let mut regions = merged;
        let mut index = 0;
        'regions: while index < regions.len() {
            let mut upward = true;
            loop {
                let region = &regions[index];
                // The whole file occurs exactly once in itself.
                if region.first == 0 && region.last == count {
                    break;
                }
                if region.first < region.last {
                    if checks == 0 {
                        regions = vec![Region {
                            first: 0,
                            last: count,
                            text: self.after.clone(),
                        }];
                        break 'regions;
                    }
                    checks -= 1;
                    let old = self.span(before, region.first, region.last);
                    if unique(&view, old, lines[region.first].start) {
                        break;
                    }
                }
                let grow_up = region.first > 0 && (upward || region.last == count);
                upward = !upward;
                if grow_up {
                    if index > 0 && regions[index - 1].last == region.first {
                        let previous = regions.remove(index - 1);
                        index -= 1;
                        let region = &mut regions[index];
                        region.first = previous.first;
                        region.text.insert_str(0, &previous.text);
                    } else {
                        let region = &mut regions[index];
                        region.first -= 1;
                        region
                            .text
                            .insert_str(0, self.span(before, region.first, region.first + 1));
                    }
                } else if index + 1 < regions.len() && regions[index + 1].first == region.last {
                    let next = regions.remove(index + 1);
                    let region = &mut regions[index];
                    region.last = next.last;
                    region.text.push_str(&next.text);
                } else {
                    let region = &mut regions[index];
                    region
                        .text
                        .push_str(self.span(before, region.last, region.last + 1));
                    region.last += 1;
                }
            }
            index += 1;
        }
        let edits: Vec<_> = regions
            .iter()
            .map(|region| FileEdit {
                old_text: self.span(before, region.first, region.last).to_owned(),
                new_text: region.text.clone(),
                replace_all: false,
            })
            .collect();
        let (after, _) = edit::apply(before, &edits, revision).ok()?;
        (after == self.after).then_some(edits)
    }

    /// Original bytes of lines `first..last`, including their endings.
    fn span<'b>(&self, before: &'b str, first: usize, last: usize) -> &'b str {
        let lines = &self.source.lines;
        &before[lines[first].start..lines[last - 1].end()]
    }
}

/// Mirrors the edit engine's single-occurrence rule, including its line-ending view.
fn unique(view: &text::View<'_>, old: &str, at: usize) -> bool {
    let pattern = text::Pattern::new(old);
    pattern.find(view, 0).is_some_and(|first| {
        pattern.range(view, first).start == at
            && pattern.find(view, pattern.following(view, first)).is_none()
    })
}
