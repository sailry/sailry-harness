use super::*;

pub(super) async fn load(
    client: Arc<Client>,
    source: impl Into<ImageSource>,
    side: u32,
    cancel: CancellationToken,
    slots: Arc<tokio::sync::Semaphore>,
) -> Result<Arc<RenderImage>, &'static str> {
    let source = source.into();
    let format = source.format().ok_or("chat_image_unavailable")?;
    if source
        .attachment()
        .is_some_and(|attachment| attachment.spec.size > sailry_protocol::attachment::MAX_BYTES)
    {
        return Err("chat_image_too_large");
    }
    let permit = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("chat_image_unavailable"),
        permit = slots.acquire_owned() => permit.map_err(|_| "chat_image_unavailable")?,
    };
    if let ImageSource::Local { source, .. } = source {
        return tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let bytes = source.image_bytes(&cancel)?;
            decode::decode(&bytes, format, side)
        })
        .await
        .map_err(|_| "chat_image_unavailable")?;
    }
    let request = client.prepare(source.command().expect("published image download"));
    let request = match &source {
        ImageSource::File {
            context: Some(context),
            ..
        } => request.with_plugin(context.clone()),
        _ => request,
    };
    let output = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("chat_image_unavailable"),
        output = client.execute(request) => output.map_err(|_| "chat_image_unavailable")?,
    };
    let mut bytes = Vec::new();
    match (&source, output) {
        (ImageSource::File { worktree, path, .. }, Output::FileDownload(download)) => {
            if download.worktree != *worktree
                || download.path != *path
                || download.size > sailry_protocol::attachment::MAX_BYTES
            {
                let _ = client
                    .execute(client.prepare(Command::CancelFileTransfer {
                        stream: download.stream,
                    }))
                    .await;
                return Err(if download.size > sailry_protocol::attachment::MAX_BYTES {
                    "chat_image_too_large"
                } else {
                    "chat_image_unavailable"
                });
            }
            client
                .download(&download, &mut bytes, cancel.clone(), |_| {})
                .await
                .map_err(|_| "chat_image_unavailable")?;
        }
        (_, Output::AttachmentDownload(download)) => {
            if source.attachment() != Some(&download.attachment) {
                let _ = client
                    .execute(client.prepare(Command::CancelFileTransfer {
                        stream: download.stream,
                    }))
                    .await;
                return Err("chat_image_unavailable");
            }
            client
                .download_attachment(&download, &mut bytes, cancel.clone(), |_| {})
                .await
                .map_err(|_| "chat_image_unavailable")?;
        }
        _ => return Err("chat_image_unavailable"),
    }
    tokio::task::spawn_blocking(move || {
        // Keep the slot until decoding really finishes, including after cancellation.
        let _permit = permit;
        if cancel.is_cancelled() {
            return Err("chat_image_unavailable");
        }
        decode::decode(&bytes, format, side)
    })
    .await
    .map_err(|_| "chat_image_unavailable")?
}
