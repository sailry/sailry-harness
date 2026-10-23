//! Conversation reads share Node ownership without granting worktree execution access.
use super::super::*;
use serde::Deserialize;
use serde_json::Value;

mod directory;
mod projection;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub caller: NodeId,
    pub session: SessionId,
    pub turn: TurnId,
}

pub(crate) enum Query {
    List(List),
    Read(Read),
    Search(Search),
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct List {
    pub text: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub before: Option<u64>,
    pub revision: Option<String>,
    pub include_current: bool,
    pub limit: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Read {
    pub session: SessionId,
    pub before: Option<TurnId>,
    pub turn: Option<TurnId>,
    pub revision: Option<u64>,
    pub before_entry: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Search {
    pub session: SessionId,
    pub text: String,
    #[serde(default)]
    pub case_sensitive: bool,
    pub before: Option<u64>,
    pub revision: Option<u64>,
    pub limit: Option<u16>,
}

impl Ingress {
    pub(crate) async fn read_session_history(
        &self,
        source: Source,
        query: Query,
    ) -> Result<Value, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ReadHistory {
                source,
                query,
                reply,
            })))
            .await
            .map_err(|_| super::super::super::unavailable())?;
        response
            .await
            .map_err(|_| super::super::super::unavailable())?
    }
}

pub(in crate::store::agent) fn visible(
    db: &Connection,
    project: Option<ProjectId>,
    target: SessionId,
) -> Result<Session, Fault> {
    let session = super::super::super::commands::session(db, target)?;
    if session.project != project {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "conversation belongs to another project",
        ));
    }
    Ok(session)
}

pub(in crate::store::agent) fn execute(
    database: &Database,
    source: Source,
    query: Query,
) -> Result<Value, Fault> {
    let db = &database.connection;
    let run = calls::active(db, source.turn)?;
    let caller: Vec<u8> = db
        .query_row(
            "SELECT caller FROM turns WHERE id=?1",
            [source.turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if run.session != source.session || caller.as_slice() != &source.caller.0[..] {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "history read does not belong to this invocation",
        ));
    }
    // Session project membership is durable metadata, independent of checkout availability.
    let session = super::super::super::commands::session(db, source.session)?;
    match query {
        Query::List(options) => directory::read(db, database.node, &session, options),
        Query::Read(options) => {
            visible(db, session.project, options.session)?;
            read(db, options)
        }
        Query::Search(options) => {
            visible(db, session.project, options.session)?;
            revision(
                db,
                options.session,
                options.revision,
                options.before.is_some(),
            )?;
            let query = sailry_protocol::conversation::search::Query {
                text: options.text,
                case_sensitive: options.case_sensitive,
                before: options.before,
                limit: options.limit.unwrap_or(20),
            };
            serde_json::to_value(search::search(db, options.session, &query)?)
                .map_err(storage_error)
        }
    }
}

fn read(db: &Connection, options: Read) -> Result<Value, Fault> {
    let limit = options.limit.unwrap_or(5);
    if !(1..=9_007_199_254_740_991).contains(&limit) {
        return Err(Fault::invalid(
            "history limit must be a positive safe integer",
        ));
    }
    let limit = limit.min(100) as u16;
    revision(
        db,
        options.session,
        options.revision,
        options.before.is_some() || options.turn.is_some(),
    )?;
    if let Some(turn) = options.turn {
        if options.before.is_some() {
            return Err(Fault::invalid("turn reads use before_entry"));
        }
        projection::turn(history::pages::turn(
            db,
            options.session,
            turn,
            options.before_entry,
            limit,
        )?)
    } else {
        if options.before_entry.is_some() {
            return Err(Fault::invalid("entry cursor requires a turn"));
        }
        projection::conversation(history::pages::read(
            db,
            options.session,
            options.before,
            limit,
        )?)
    }
}

fn revision(
    db: &Connection,
    session: SessionId,
    expected: Option<u64>,
    required: bool,
) -> Result<(), Fault> {
    if required && expected.is_none() {
        return Err(Fault::invalid("history continuations require a revision"));
    }
    if let Some(expected) = expected {
        super::super::super::commands::check_revision(
            super::super::super::sessions::history_revision(db, session)?,
            expected,
        )?;
    }
    Ok(())
}
