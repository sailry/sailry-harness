//! Durable package callbacks observe terminal turns without another execution loop.
use super::*;
use sailry_link::Handler as _;
use sailry_protocol::{Command, NodeId, Request, plugin};

pub(in crate::agent) struct Callback {
    caller: NodeId,
    context: plugin::Context,
    handler: String,
    turn: sailry_protocol::TurnId,
    automatic: bool,
    names: Vec<String>,
}

pub(in crate::agent) fn capture(invocation: &Invocation) -> Vec<Callback> {
    if invocation.turn.kind != sailry_protocol::conversation::RunKind::Task
        || invocation.child.is_some()
    {
        return Vec::new();
    }
    invocation
        .plugins
        .iter()
        .filter_map(|package| {
            let handler = package
                .extension
                .as_ref()?
                .host
                .as_ref()?
                .turn
                .as_ref()?
                .after_turn
                .clone()?;
            Some(Callback {
                caller: invocation.caller,
                turn: invocation.turn.id,
                automatic: invocation.automatic,
                names: package
                    .extension
                    .as_ref()?
                    .tools
                    .iter()
                    .map(|tool| crate::plugins::tools::alias(&package.summary.name, &tool.name))
                    .collect(),
                context: plugin::Context {
                    invocation: None,
                    turn: None,
                    surface: plugin::desktop::Surface::Workspace,
                    package: package.summary.reference(),
                    worktree: Some(invocation.worktree),
                    session: Some(invocation.turn.session),
                },
                handler,
            })
        })
        .collect()
}

pub(in crate::agent) async fn complete(
    ingress: &Arc<Ingress>,
    callbacks: Vec<Callback>,
    status: sailry_protocol::conversation::Status,
) {
    for callback in callbacks {
        let request = Request::new(ingress.node, Command::CallPlugin {
            handler: callback.handler,
            input: serde_json::json!({"turn":callback.turn,"status":status,"automatic":callback.automatic,"names":callback.names}),
        }).with_plugin(callback.context);
        let result = match ingress.dispatch(callback.caller, request).await {
            Ok(admitted) => admitted.completion.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "turn callback completion is unknown",
                ))
            }),
            Err(fault) => Err(fault),
        };
        if let Err(fault) = result
            && !matches!(
                fault.code,
                ErrorCode::NotConfigured | ErrorCode::NotFound | ErrorCode::RevisionConflict
            )
        {
            eprintln!("Plugin turn callback failed: {fault}");
        }
    }
}
