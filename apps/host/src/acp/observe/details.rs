//! Canonical values supplement already published summary and permission updates.
use super::*;
use sailry_protocol::conversation::Page;
use std::time::Duration;

pub(super) fn start(
    adapter: Arc<Adapter>,
    opened: Arc<Opened>,
    source: SessionId,
    call: Call,
    page: Arc<Page>,
    stamp: Stamp,
    mut receiver: watch::Receiver<View>,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let connected = connection.clone();
    connection.spawn(async move {
        let id = call_id(source, &call);
        let current = async {
            loop {
                let matches = {
                    let view = receiver.borrow_and_update();
                    view.snapshot.as_ref().is_some_and(|snapshot| {
                        view.calls.iter().any(|call| call_id(source, call) == id
                            && call.stamp(&snapshot.page).as_ref() == Some(&stamp))
                    })
                };
                if !matches || receiver.changed().await.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            _ = opened.stop.cancelled() => Ok(()),
            _ = current => Ok(()),
            result = supplement(&adapter, &opened, source, &call, &page, &stamp, &id, &connected) => result,
        }
    })
}

async fn supplement(
    adapter: &Adapter,
    opened: &Opened,
    source: SessionId,
    call: &Call,
    page: &Page,
    stamp: &Stamp,
    id: &str,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    for delay in [
        Duration::ZERO,
        Duration::from_millis(200),
        Duration::from_secs(1),
        Duration::from_secs(3),
    ] {
        tokio::time::sleep(delay).await;
        if opened.presentation.lock().await.stamps.get(id) != Some(stamp) {
            return Ok(());
        }
        let permit = adapter
            .details
            .acquire()
            .await
            .map_err(|_| invalid("tool details are closed"))?;
        let result = adapter.client.tool_detail(stamp.reference.clone()).await;
        drop(permit);
        let detail = match result {
            Ok(detail) => detail,
            Err(error) => {
                tracing::debug!(session = %source, call = %call.source.key(), %error.message, "tool detail read failed");
                continue;
            }
        };
        let session = acp::SessionId::new(opened.session.lock().await.id.to_string());
        let presentation = opened.presentation.lock().await;
        if presentation.stamps.get(id) != Some(stamp) {
            return Ok(());
        }
        let call = call.with_detail(detail);
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
        return Ok(());
    }
    Ok(())
}
