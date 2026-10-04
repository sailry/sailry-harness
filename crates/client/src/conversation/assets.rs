//! Shared pagination for conversation resource and artifact metadata.
use super::projection::invalid;
use crate::Client;
use sailry_protocol::{
    conversation::assets::{self, Query},
    *,
};

impl Client {
    pub async fn conversation_assets(
        &self,
        session: SessionId,
        mut query: Query,
    ) -> Result<assets::Page, Fault> {
        if !(1..=assets::MAX_GROUPS).contains(&query.limit) {
            return Err(invalid("conversation asset limit is invalid"));
        }
        let limit = usize::from(query.limit);
        let mut result = assets::Page {
            session,
            revision: 0,
            groups: Vec::new(),
            next_before: None,
        };
        loop {
            query.limit = (limit - result.groups.len()) as u16;
            let Output::ConversationAssets(page) = self
                .execute(self.prepare(Command::ListConversationAssets {
                    session,
                    query: query.clone(),
                }))
                .await?
            else {
                return Err(invalid("conversation asset response expected"));
            };
            validate(&page, session, &query)?;
            if result.revision != 0 && result.revision != page.revision {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "conversation history changed while reading assets",
                ));
            }
            result.revision = page.revision;
            result.next_before = page.next_before;
            result.groups.extend(page.groups);
            if result.groups.len() == limit || result.next_before.is_none() {
                return Ok(result);
            }
            query.before = result.next_before;
        }
    }
}

fn validate(page: &assets::Page, session: SessionId, query: &Query) -> Result<(), Fault> {
    if page.session != session || page.revision == 0 || page.groups.len() > usize::from(query.limit)
    {
        return Err(invalid(
            "conversation asset response scope or size is invalid",
        ));
    }
    let mut before = query.before.unwrap_or(i64::MAX as u64);
    for group in &page.groups {
        if group.sequence == 0
            || group.sequence >= before
            || group.items.is_empty()
            || query.kind.is_some_and(|kind| kind != group.kind)
        {
            return Err(invalid("conversation asset group is invalid"));
        }
        before = group.sequence;
    }
    if page.next_before.is_some_and(|next| {
        next == 0 || next > before || next >= query.before.unwrap_or(i64::MAX as u64)
    }) {
        return Err(invalid("conversation asset cursor did not advance"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
