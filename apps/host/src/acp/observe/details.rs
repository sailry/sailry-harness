//! Canonical values supplement already published summary and permission updates.
use super::*;
use sailry_client::conversation::details::State;

pub(super) fn start(
    adapter: Arc<Adapter>,
    opened: Arc<Opened>,
    source: SessionId,
    call: Call,
    stamp: Stamp,
    receiver: watch::Receiver<View>,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let connected = connection.clone();
    connection.spawn(async move {
        let id = call_id(source, &call);
        let result=tokio::select! {
            _ = opened.stop.cancelled() => Ok(()),
            result = supplement(&adapter, &opened, source, &stamp, &id, &receiver, &connected) => result,
        };
        opened.details.remove(&stamp.reference);
        result
    })
}

async fn supplement(
    adapter: &Adapter,
    opened: &Opened,
    source: SessionId,
    stamp: &Stamp,
    id: &str,
    receiver: &watch::Receiver<View>,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let State::Ready(detail) = opened.details.wait(&stamp.reference).await else {
        tracing::debug!(session=%source,call=%id,"tool detail supplement is unavailable");
        return Ok(());
    };
    let session = acp::SessionId::new(opened.session.lock().await.id.to_string());
    let presentation = opened.presentation.lock().await;
    let latest = receiver.borrow().clone();
    let Some(snapshot) = &latest.snapshot else {
        return Ok(());
    };
    let page = &snapshot.page;
    let Some(call) = latest.calls.iter().find(|call| {
        call_id(source, call) == id
            && call.detail_reference(page).as_ref() == Some(&stamp.reference)
    }) else {
        return Ok(());
    };
    if presentation
        .stamps
        .get(id)
        .is_none_or(|current| current.reference != stamp.reference)
    {
        return Ok(());
    }
    let call = call.with_detail(detail.detail.clone());
    let mut context = context(source, &call);
    context["node"] = json!(adapter.client.target());
    let update = acp::ToolCallUpdate::new(
        id.to_owned(),
        acp::ToolCallUpdateFields::new()
            .status(status(&call, page))
            .raw_input(call.arguments(page).cloned())
            .raw_output(call.result(page).cloned()),
    );
    notify(
        connection,
        &session,
        acp::SessionUpdate::ToolCallUpdate(update),
        context,
    )?;
    Ok(())
}
