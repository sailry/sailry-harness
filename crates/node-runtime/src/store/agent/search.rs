use super::*;
use regex::{Regex, RegexBuilder};
use sailry_protocol::conversation::search::{self, Query};

#[cfg(test)]
mod tests;

const MAX_EVENTS: usize = 256;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const CONTEXT: usize = 96;

pub(in crate::store) fn search(
    db: &Connection,
    session: SessionId,
    options: &Query,
) -> Result<search::Page, Fault> {
    check_session(db, session)?;
    if options.text.trim().is_empty()
        || options.text.len() > search::MAX_QUERY_BYTES
        || !(1..=search::MAX_MATCHES).contains(&options.limit)
        || options
            .before
            .is_some_and(|sequence| sequence == 0 || sequence > i64::MAX as u64)
    {
        return Err(invalid("conversation search bounds are invalid"));
    }
    if let Some(before) = options.before {
        let visible: Option<bool> = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_turns h WHERE h.session=?2 AND h.turn=e.turn) FROM agent_events e WHERE e.sequence=?1",
                params![before as i64, session.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        match visible {
            Some(true) => {}
            Some(false) => {
                return Err(Fault::new(
                    ErrorCode::WrongTarget,
                    "search cursor belongs to another session",
                ));
            }
            None => {
                return Err(Fault::new(
                    ErrorCode::NotFound,
                    "search cursor no longer exists",
                ));
            }
        }
    }
    let matcher = RegexBuilder::new(&regex::escape(&options.text))
        .case_insensitive(!options.case_sensitive)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|_| invalid("conversation search text is invalid"))?;
    let mut query = db.prepare(history::EVENTS_QUERY).map_err(storage_error)?;
    let mut rows = query
        .query(params![
            session.to_string(),
            options.before.unwrap_or(i64::MAX as u64) as i64,
            (MAX_EVENTS + 1) as i64,
        ])
        .map_err(storage_error)?;
    let mut page = search::Page {
        session,
        revision: crate::store::sessions::history_revision(db, session)?,
        matches: Vec::new(),
        next_before: None,
    };
    let mut count = 0;
    let mut bytes = 0;
    let mut last = None;
    while let Some(row) = rows.next().map_err(storage_error)? {
        if count == MAX_EVENTS
            || bytes >= MAX_BYTES
            || page.matches.len() == usize::from(options.limit)
        {
            page.next_before = last;
            break;
        }
        let sequence = row.get::<_, i64>(0).map_err(storage_error)? as u64;
        let body: Vec<u8> = row.get(2).map_err(storage_error)?;
        bytes += body.len();
        count += 1;
        last = Some(sequence);
        let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
        if let Some((part, snippet, highlight)) = find(&event, &matcher) {
            let turn: String = row.get(1).map_err(storage_error)?;
            page.matches.push(search::Match {
                turn: turn.parse().map_err(storage_error)?,
                turn_sequence: row.get::<_, i64>(3).map_err(storage_error)? as u64,
                entry: event.id,
                sequence,
                part,
                author: event.author,
                snippet,
                highlight,
            });
        }
    }
    Ok(page)
}

fn find(event: &AdkEvent, matcher: &Regex) -> Option<(usize, String, std::ops::Range<usize>)> {
    event
        .content()?
        .parts
        .iter()
        .enumerate()
        .find_map(|(index, part)| {
            let adk_core::Part::Text { text } = part else {
                return None;
            };
            let matched = matcher.find(text)?;
            let start = text[..matched.start()]
                .char_indices()
                .rev()
                .nth(CONTEXT - 1)
                .map_or(0, |(index, _)| index);
            let end = text[matched.end()..]
                .char_indices()
                .nth(CONTEXT)
                .map_or(text.len(), |(index, _)| matched.end() + index);
            Some((
                index,
                text[start..end].into(),
                matched.start() - start..matched.end() - start,
            ))
        })
}
