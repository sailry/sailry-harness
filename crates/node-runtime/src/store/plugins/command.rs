//! Declared slash commands route through an ordinary scoped plugin callback.
use super::*;
use crate::store::{commands, database::Database};
use sailry_protocol::{NodeId, Request};

pub(in crate::store) fn resolve(
    db: &Database,
    request: &Request,
) -> Result<Option<Request>, Fault> {
    if request.plugin.is_some() {
        return Ok(None);
    }
    let Command::SubmitTurn {
        session,
        expected_revision,
        message,
    } = &request.command
    else {
        return Ok(None);
    };
    let text = message.text.trim_start();
    let Some(text) = text.strip_prefix('/') else {
        return Ok(None);
    };
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    let name = &text[..end];
    let session = commands::session(&db.connection, *session)?;
    commands::check_revision(session.revision, *expected_revision)?;
    crate::store::sessions::writable(&session)?;
    let references = if let Some(assistant) = &session.config.assistant {
        crate::store::agent::plugins::assistant(&db.connection, assistant)?
    } else {
        crate::store::agent::plugins::capture(&db.connection)?
    };
    let packages = crate::store::agent::plugins::packages(&db.connection, &references)?;
    let mut selected = None;
    for package in packages {
        let Some(command) = package
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .and_then(|host| host.commands.iter().find(|command| command.name == name))
        else {
            continue;
        };
        if selected.is_some() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "slash command has multiple providers",
            ));
        }
        let mut child = Request::new(
            db.node,
            Command::CallPlugin {
                handler: command.handler.clone(),
                input: serde_json::json!({"arguments":text[end..].trim(),"message":message}),
            },
        )
        .with_plugin(plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Workspace,
            package: package.summary.reference(),
            worktree: Some(session.worktree),
            session: Some(session.id),
        });
        let mut identity = blake3::Hasher::new_derive_key("Sailry plugin command v1");
        identity.update(
            &serde_json::to_vec(&(request.id, child.plugin.as_ref())).map_err(storage_error)?,
        );
        let mut bytes: [u8; 16] = identity.finalize().as_bytes()[..16].try_into().unwrap();
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        child.id = sailry_protocol::RequestId::try_from(uuid::Uuid::from_bytes(bytes))
            .map_err(storage_error)?;
        selected = Some(child);
    }
    Ok(selected)
}

pub(in crate::store) async fn run(
    ingress: &std::sync::Arc<crate::store::Ingress>,
    caller: NodeId,
    child: Request,
) -> Result<Output, Fault> {
    use sailry_link::Handler as _;
    let session = child.plugin.as_ref().and_then(|context| context.session);
    let output = ingress
        .dispatch(caller, child)
        .await?
        .completion
        .await
        .map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "slash command completion is unknown",
            )
        })??;
    let Output::PluginResult(value) = output else {
        return Err(Fault::new(
            ErrorCode::Internal,
            "invalid command callback output",
        ));
    };
    if value["kind"] == "queued_turn" {
        let Output::QueuedTurn(turn) = serde_json::from_value::<Output>(value)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid command turn output"))?
        else {
            unreachable!()
        };
        if Some(turn.session) != session {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "command turn belongs to another session",
            ));
        }
        Ok(Output::QueuedTurn(turn))
    } else {
        Ok(Output::PluginResult(value))
    }
}
