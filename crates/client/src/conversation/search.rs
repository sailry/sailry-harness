use crate::Client;
use sailry_protocol::{
    conversation::search::{self, Query},
    *,
};

use std::ops::Range;

#[cfg(test)]
mod tests;

impl Client {
    /// Fill a logical result page across bounded Node scans; cancellation drops this read-only work.
    pub async fn search_conversation(
        &self,
        session: SessionId,
        mut query: Query,
    ) -> Result<search::Page, Fault> {
        if !(1..=search::MAX_MATCHES).contains(&query.limit) {
            return Err(sailry_protocol::Fault::invalid(
                "conversation search result limit is invalid",
            ));
        }
        let limit = usize::from(query.limit);
        let mut result = search::Page {
            session,
            revision: 0,
            matches: Vec::new(),
            next_before: None,
        };
        let mut identities = std::collections::BTreeSet::new();
        loop {
            query.limit = (limit - result.matches.len()) as u16;
            let Output::ConversationMatches(page) = self
                .execute(self.prepare(Command::SearchConversation {
                    session,
                    query: query.clone(),
                }))
                .await?
            else {
                return Err(sailry_protocol::Fault::invalid(
                    "conversation search response expected",
                ));
            };
            validate(&page, session, &query)?;
            if result.revision != 0 && result.revision != page.revision {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "conversation history changed during search",
                ));
            }
            result.revision = page.revision;
            if page
                .matches
                .iter()
                .any(|found| !identities.insert(found.entry.clone()))
            {
                return Err(sailry_protocol::Fault::invalid(
                    "conversation search repeated an entry across scans",
                ));
            }
            result.next_before = page.next_before;
            result.matches.extend(page.matches);
            if result.matches.len() == limit || result.next_before.is_none() {
                return Ok(result);
            }
            query.before = result.next_before;
        }
    }
}

fn validate(page: &search::Page, session: SessionId, query: &Query) -> Result<(), Fault> {
    if page.revision == 0
        || page.session != session
        || page.matches.len() > usize::from(query.limit)
    {
        return Err(sailry_protocol::Fault::invalid(
            "search response belongs to another read or exceeds its limit",
        ));
    }
    let mut before = query.before.unwrap_or(i64::MAX as u64);
    let mut identities = std::collections::BTreeSet::new();
    for found in &page.matches {
        if found.sequence == 0
            || found.sequence >= before
            || found.turn_sequence == 0
            || found.entry.is_empty()
            || !identities.insert(&found.entry)
            || found.snippet.len() > search::MAX_SNIPPET_BYTES
            || found.highlight.start >= found.highlight.end
            || found.snippet.get(found.highlight.clone()).is_none()
        {
            return Err(sailry_protocol::Fault::invalid(
                "conversation search match is invalid",
            ));
        }
        before = found.sequence;
    }
    if page.next_before.is_some_and(|next| {
        next == 0 || next > before || next >= query.before.unwrap_or(i64::MAX as u64)
    }) {
        return Err(sailry_protocol::Fault::invalid(
            "conversation search cursor did not advance",
        ));
    }
    Ok(())
}

/// One-line plain text for a match row, with the highlight remapped onto it.
///
/// Inline Markdown syntax and block markers are removed and whitespace is collapsed. When the
/// match lies entirely inside removed syntax, the raw snippet and highlight are returned.
pub fn display(found: &search::Match) -> (String, Range<usize>) {
    let raw = || (found.snippet.clone(), found.highlight.clone());
    let mut kept = Vec::with_capacity(found.snippet.len());
    let mut offset = 0;
    for line in found.snippet.split_inclusive('\n') {
        plain(line, offset, &mut kept);
        offset += line.len();
    }
    let mut text = String::with_capacity(found.snippet.len());
    let mut highlight: Option<Range<usize>> = None;
    let mut space = false;
    for (character, origin) in kept {
        if character.is_whitespace() {
            space = !text.is_empty();
            continue;
        }
        if std::mem::take(&mut space) {
            text.push(' ');
        }
        let start = text.len();
        text.push(character);
        if found.highlight.contains(&origin) {
            let range = highlight.get_or_insert(start..start);
            range.end = text.len();
        }
    }
    highlight.map_or_else(raw, |highlight| (text, highlight))
}

/// Keeps a line's visible characters with their snippet byte offsets.
fn plain(line: &str, offset: usize, kept: &mut Vec<(char, usize)>) {
    let mut start = line.len() - line.trim_start().len();
    loop {
        let length = marker(&line[start..]);
        if length == 0 {
            break;
        }
        start += length;
        start += line[start..].len() - line[start..].trim_start().len();
    }
    let body = &line[start..];
    let content = body.trim();
    // Table delimiter rows and thematic breaks carry no readable text.
    if !content.is_empty()
        && content
            .chars()
            .all(|character| matches!(character, '|' | '-' | ':' | ' ' | '\t'))
    {
        kept.push((' ', offset + start));
        return;
    }
    inline(body, offset + start, kept);
}

/// Byte length of one leading block marker: heading, quote, bullet or ordinal.
fn marker(line: &str) -> usize {
    let spaced = |length: usize| {
        if line[length..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
        {
            length
        } else {
            0
        }
    };
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&hashes) {
        return spaced(hashes);
    }
    if line.starts_with('>') {
        return 1;
    }
    if line.starts_with(['-', '*', '+']) {
        return spaced(1);
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) && line[digits..].starts_with(['.', ')']) {
        return spaced(digits + 1);
    }
    0
}

/// Removes emphasis, code, strike-through and link syntax; table pipes become spaces.
fn inline(text: &str, offset: usize, kept: &mut Vec<(char, usize)>) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |index: usize| chars.get(index).map(|(_, character)| *character);
    let mut code = false;
    let mut links: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while let Some((position, character)) = chars.get(index).copied() {
        let previous = index.checked_sub(1).and_then(at);
        let next = at(index + 1);
        index += 1;
        if character == '`' {
            code = !code;
            continue;
        }
        if code {
            kept.push((character, offset + position));
            continue;
        }
        let word = |side: Option<char>| side.is_some_and(|side| !side.is_whitespace());
        match character {
            '\\' if next.is_some_and(|next| next.is_ascii_punctuation()) => {
                kept.push((chars[index].1, offset + chars[index].0));
                index += 1;
            }
            '*' if word(previous) || word(next) => {}
            '_' if (word(previous) || word(next))
                && !(previous.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric)) => {}
            '~' if previous == Some('~') || next == Some('~') => {}
            '|' => kept.push((' ', offset + position)),
            '!' if next == Some('[') && link(&chars, index).is_some() => {}
            '[' => match link(&chars, index - 1) {
                Some(target) => links.push(target),
                None => kept.push((character, offset + position)),
            },
            ']' if links.last().is_some_and(|(close, _)| *close == index - 1) => {
                let (_, end) = links.pop().unwrap_or_default();
                index = end + 1;
            }
            _ => kept.push((character, offset + position)),
        }
    }
}

/// Char indices of the `]` and `)` closing an inline link that opens at `open`.
fn link(chars: &[(usize, char)], open: usize) -> Option<(usize, usize)> {
    let close = open
        + 1
        + chars[open + 1..]
            .iter()
            .position(|(_, character)| *character == ']')?;
    if chars.get(close + 1)?.1 != '(' {
        return None;
    }
    let end = close
        + 2
        + chars[close + 2..]
            .iter()
            .position(|(_, character)| *character == ')')?;
    Some((close, end))
}
