use super::*;
use sailry_link::Admission;
use sailry_protocol::{Command, Fault, NodeId, Output, Request};

pub(crate) fn begin(node: NodeId, caller: NodeId, request: &Request) -> Option<Span> {
    let (kind, session, turn) = match &request.command {
        Command::SubmitTurn { session, .. } => ("submit_turn", Some(*session), None),
        Command::QueueTurn { session, .. } => ("queue_turn", Some(*session), None),
        Command::SubmitTask { session, .. } => ("submit_task", Some(*session), None),
        Command::GuideTurn { recipient, .. } => ("guide_turn", None, Some(*recipient)),
        Command::GuideQueuedTurn { turn, .. } => ("guide_queued_turn", None, Some(*turn)),
        Command::StopTurn { turn } => ("stop_turn", None, Some(*turn)),
        Command::ResolveQuestion { session, .. } => ("resolve_question", Some(*session), None),
        Command::ResolveApproval { session, .. } => ("resolve_approval", Some(*session), None),
        Command::ReadConversation { session, .. } => ("read_conversation", Some(*session), None),
        Command::ReadTurnCapture { session, turn } => {
            ("read_turn_capture", Some(*session), Some(*turn))
        }
        Command::ReadTurn { session, turn, .. } => ("read_turn", Some(*session), Some(*turn)),
        _ if request.plugin.is_some() => (
            "plugin_command",
            request.plugin.as_ref().and_then(|context| context.session),
            request.plugin.as_ref().and_then(|context| context.turn),
        ),
        _ => return None,
    };
    let mut entry = Entry::new(Area::Command, Phase::Begin);
    entry.command = Some(kind);
    entry.context = Context {
        node: Some(node),
        caller: Some(caller),
        request: Some(request.id),
        session,
        turn,
        ..Default::default()
    };
    if let Some(context) = &request.plugin {
        entry.context.package = Some(context.package.clone());
        entry.context.invocation = context.invocation;
    }
    if let Command::RequestProviderHttp { provider, .. } = &request.command {
        entry.stage = Some("provider_http");
        entry.context.provider = Some(*provider);
    }
    Some(Span::new(entry))
}

pub(crate) fn admitted(mut span: Span, mut admission: Admission) -> Admission {
    span.entry.durable = Some(admission.receipt.durable);
    span.record(Phase::Admitted);
    let (sender, completion) = tokio::sync::oneshot::channel();
    let original = std::mem::replace(&mut admission.completion, completion);
    tokio::spawn(async move {
        match original.await {
            Ok(result) => {
                match &result {
                    Ok(output) => {
                        if let Output::QueuedTurn(turn) = output {
                            span.entry.context.session = Some(turn.session);
                            span.entry.context.turn = Some(turn.id);
                        }
                        if span.entry.stage == Some("provider_http")
                            && let Output::PluginHttp(response) = output
                        {
                            span.entry.upstream_status = Some(u64::from(response.status));
                            span.entry.reason = Some(if (200..300).contains(&response.status) {
                                "http_success"
                            } else {
                                "http_rejected"
                            });
                        }
                        span.finish(Phase::Completed, None);
                    }
                    Err(fault) => span.finish(Phase::Failed, Some(fault.code)),
                }
                let _ = sender.send(result);
            }
            Err(_) => {
                span.finish(
                    Phase::Interrupted,
                    Some(sailry_protocol::ErrorCode::OutcomeUnknown),
                );
                // Drop the sender: preserve the original completion channel closure.
            }
        }
    });
    admission
}

pub(crate) fn rejected(span: &mut Span, fault: &Fault) {
    span.finish(Phase::Failed, Some(fault.code));
}
