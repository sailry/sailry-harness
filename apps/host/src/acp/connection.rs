use super::*;
use sdk::{Agent, ConnectTo, Responder, schema::ProtocolVersion};

struct Closing(CancellationToken);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) async fn serve<C>(adapter: Arc<Adapter>, component: C) -> acp::Result<()>
where
    C: ConnectTo<Agent> + 'static,
{
    let _closing = Closing(adapter.stop.clone());
    let closing = adapter.stop.clone();
    let builder = Agent
        .builder()
        .name("Sailry")
        .on_close(async move |_connection| {
            closing.cancel();
            Ok(())
        })
        .on_receive_request(
            async |_: acp::InitializeRequest,
                   responder: Responder<acp::InitializeResponse>,
                   _connection: ConnectionTo<sdk::Client>| {
                let sessions = acp::SessionCapabilities::new()
                    .list(acp::SessionListCapabilities::new())
                    .resume(acp::SessionResumeCapabilities::new())
                    .fork(acp::SessionForkCapabilities::new())
                    .close(acp::SessionCloseCapabilities::new());
                responder.respond(
                    acp::InitializeResponse::new(ProtocolVersion::V1)
                        .agent_info(acp::Implementation::new(
                            "Sailry",
                            env!("CARGO_PKG_VERSION"),
                        ))
                        .agent_capabilities(
                            acp::AgentCapabilities::new()
                                .load_session(true)
                                .session_capabilities(sessions),
                        ),
                )
            },
            sdk::on_receive_request!(),
        );

    macro_rules! route {
        ($builder:expr, $method:ident, $request:ty, $response:ty) => {{
            let adapter = adapter.clone();
            $builder.on_receive_request(
                move |request: $request,
                      responder: Responder<$response>,
                      connection: ConnectionTo<sdk::Client>| {
                    let adapter = adapter.clone();
                    let spawned = connection.clone();
                    async move {
                        connection.spawn(async move {
                            let result = adapter.$method(request, &spawned).await;
                            responder.respond_with_result(result)
                        })?;
                        Ok(())
                    }
                },
                sdk::on_receive_request!(),
            )
        }};
    }

    let builder = route!(
        builder,
        new_session,
        acp::NewSessionRequest,
        acp::NewSessionResponse
    );
    let builder = route!(
        builder,
        load_session,
        acp::LoadSessionRequest,
        acp::LoadSessionResponse
    );
    let builder = route!(
        builder,
        resume_session,
        acp::ResumeSessionRequest,
        acp::ResumeSessionResponse
    );
    let builder = route!(
        builder,
        fork_session,
        acp::ForkSessionRequest,
        acp::ForkSessionResponse
    );
    let builder = route!(
        builder,
        list_sessions,
        acp::ListSessionsRequest,
        acp::ListSessionsResponse
    );
    let builder = route!(
        builder,
        close_session,
        acp::CloseSessionRequest,
        acp::CloseSessionResponse
    );
    let builder = route!(
        builder,
        set_mode,
        acp::SetSessionModeRequest,
        acp::SetSessionModeResponse
    );
    let builder = route!(
        builder,
        set_config,
        acp::SetSessionConfigOptionRequest,
        acp::SetSessionConfigOptionResponse
    );
    let prompting = adapter.clone();
    let builder = builder.on_receive_request(
        move |request: acp::PromptRequest,
              responder: Responder<acp::PromptResponse>,
              connection: ConnectionTo<sdk::Client>| {
            let adapter = prompting.clone();
            let spawned = connection.clone();
            async move {
                let cancelled = responder.cancellation();
                connection.spawn(async move {
                    responder
                        .respond_with_result(adapter.prompt(request, &spawned, cancelled).await)
                })?;
                Ok(())
            }
        },
        sdk::on_receive_request!(),
    );
    let cancel = adapter.clone();
    let result = builder
        .on_receive_notification(
            async move |notification: acp::CancelNotification,
                        _connection: ConnectionTo<sdk::Client>| {
                cancel.cancel(notification).await
            },
            sdk::on_receive_notification!(),
        )
        .connect_to(component)
        .await;
    adapter.stop.cancel();
    result
}
