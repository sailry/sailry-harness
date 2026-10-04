//! Controller secret reads terminate in the native field, never a script result.
use super::*;

pub(in crate::plugins) fn load(
    fields: Entity<Store>,
    id: String,
    job: tokio::task::JoinHandle<Result<Option<Secret>, HostError>>,
) -> Result<impl std::future::Future<Output = Result<bool, HostError>> + Send + 'static, HostError>
{
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        fields
            .update(cx, |fields, _| {
                let draft = fields.drafts.get_mut(&id).ok_or_else(missing)?;
                draft.loading += 1;
                Ok::<_, HostError>((draft.version, draft.loading))
            })
            .map(|(version, loading)| {
                cx.defer(move |cx| {
                    fields.update(cx, |_, cx| {
                        cx.spawn(async move |fields, cx| {
                            let result = job
                                .await
                                .map_err(|_| HostError::new("credential request worker failed"))
                                .and_then(|result| result);
                            let result = fields
                                .update(cx, |fields, cx| {
                                    let draft = fields.drafts.get_mut(&id).ok_or_else(missing)?;
                                    if draft.version != version || draft.loading != loading {
                                        return Ok(false);
                                    }
                                    draft.pending =
                                        Some(result?.unwrap_or_else(|| Secret::new(String::new())));
                                    draft.imported = None;
                                    cx.notify();
                                    Ok(true)
                                })
                                .unwrap_or_else(|_| Err(missing()));
                            let _ = reply.send(result);
                        })
                        .detach();
                    });
                });
            })
    })
    .ok_or_else(missing)??;
    Ok(async move { receive.await.map_err(|_| missing())? })
}
