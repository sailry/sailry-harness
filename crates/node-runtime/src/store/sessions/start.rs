//! Creation and first-turn admission share the existing command transaction.
use super::*;

pub(in crate::store) async fn start(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    profile: Option<&std::path::Path>,
    request: &Request,
    draft: &conversation::Start,
) -> Result<(Output, Option<Event>), Fault> {
    let config = match &draft.config {
        Some(config) => config.clone(),
        None => commands::defaults(db).await?.config.ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "Node model defaults are not configured",
            )
        })?,
    };
    let roles = roles::capture(db, node, super::super::roles::list(db).await?).await?;
    let (Output::Session(session), _) = create(
        db,
        node,
        profile,
        draft.project,
        draft.worktree,
        Configuration {
            config,
            profile: None,
            roles,
        },
    )
    .await?
    else {
        unreachable!()
    };
    let (Output::Session(session), _) =
        rename(db, session.id, session.revision, &draft.title).await?
    else {
        unreachable!()
    };
    let turn =
        super::super::agent::queue::create(db, caller, request, session, &draft.message, true)
            .await?;
    let session = commands::session(db, turn.session).await?;
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(Event::TaskStarted {
            session: Box::new(session),
            turn,
        }),
    ))
}
