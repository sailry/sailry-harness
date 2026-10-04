use super::projection::invalid;
use crate::Client;
use sailry_protocol::{
    conversation::search::{self, Query},
    *,
};

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
            return Err(invalid("conversation search result limit is invalid"));
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
                return Err(invalid("conversation search response expected"));
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
                return Err(invalid(
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
        return Err(invalid(
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
            return Err(invalid("conversation search match is invalid"));
        }
        before = found.sequence;
    }
    if page.next_before.is_some_and(|next| {
        next == 0 || next > before || next >= query.before.unwrap_or(i64::MAX as u64)
    }) {
        return Err(invalid("conversation search cursor did not advance"));
    }
    Ok(())
}
