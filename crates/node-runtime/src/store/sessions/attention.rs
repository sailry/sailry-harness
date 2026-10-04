//! Durable session attention, independent of configuration and controller inboxes.
use super::*;
use sailry_protocol::activity::Attention;

pub(in crate::store) fn read(db: &Connection, id: SessionId) -> Result<Attention, Fault> {
    db.query_row(
        "SELECT revision,unread FROM session_attention WHERE session=?1",
        [id.to_string()],
        |row| {
            Ok(Attention {
                revision: row.get::<_, i64>(0)? as u64,
                unread: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(storage_error)
    .map(Option::unwrap_or_default)
}

pub(in crate::store) fn notify(db: &Connection, id: SessionId) -> Result<(), Fault> {
    write(db, id, true)
}

fn write(db: &Connection, id: SessionId, unread: bool) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO session_attention(session,revision,unread) VALUES(?1,1,?2)
         ON CONFLICT(session) DO UPDATE SET revision=revision+1,unread=excluded.unread",
        params![id.to_string(), unread],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn set(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
    acknowledged: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id)?;
    let attention = session.activity.attention;
    if attention.unread == acknowledged {
        commands::check_revision(attention.revision, expected_revision)?;
        write(db, id, !acknowledged)?;
        session.activity.attention = read(db, id)?;
        let event = Event::ConversationChanged {
            session: id,
            activity: session.activity.clone(),
        };
        Ok((Output::Session(session), Some(event)))
    } else {
        Ok((Output::Session(session), None))
    }
}
