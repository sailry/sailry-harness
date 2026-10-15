use super::*;
use sailry_node_runtime::logging::{Area, Context as LogContext, Entry, Phase};
use sailry_protocol::ErrorCode;
use std::time::Instant;

pub(super) async fn load(
    client: Arc<Client>,
    source: impl Into<ImageSource>,
    side: u32,
    cancel: CancellationToken,
    slots: Arc<tokio::sync::Semaphore>,
) -> Result<Arc<RenderImage>, &'static str> {
    let source = source.into();
    let started = Instant::now();
    let mut diagnostic = Entry::new(Area::Image, Phase::Begin);
    diagnostic.context = LogContext {
        node: Some(client.target()),
        ..Default::default()
    };
    diagnostic.source_kind = Some(match &source {
        ImageSource::File {
            worktree, context, ..
        } => {
            diagnostic.context.worktree = Some(*worktree);
            diagnostic.context.package = context.as_ref().map(|context| context.package.clone());
            "file"
        }
        ImageSource::Attachment(attachment) => {
            diagnostic.context.worktree = Some(attachment.spec.worktree);
            "attachment"
        }
        ImageSource::Image { session, .. } => {
            diagnostic.context.session = Some(*session);
            "generated"
        }
        ImageSource::Local { .. } => "local",
    });
    diagnostic.emit();
    let kind = source.kind().ok_or_else(|| {
        failed(
            &diagnostic,
            started,
            "source_kind",
            ErrorCode::InvalidRequest,
            None,
            "chat_image_unavailable",
        )
    })?;
    if source
        .attachment()
        .is_some_and(|attachment| attachment.spec.size > sailry_protocol::attachment::MAX_BYTES)
    {
        return Err(failed(
            &diagnostic,
            started,
            "size",
            ErrorCode::InvalidRequest,
            None,
            "chat_image_too_large",
        ));
    }
    let permit = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(failed(&diagnostic, started, "slot", ErrorCode::Cancelled, None, "chat_image_unavailable")),
        permit = slots.acquire_owned() => permit.map_err(|_| failed(&diagnostic, started, "slot", ErrorCode::Unavailable, None, "chat_image_unavailable"))?,
    };
    if let ImageSource::Local { source, .. } = source {
        let decoding = diagnostic.clone();
        return tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let bytes = source.image_bytes(&cancel).map_err(|key| {
                failed(
                    &decoding,
                    started,
                    "local_read",
                    if cancel.is_cancelled() {
                        ErrorCode::Cancelled
                    } else {
                        ErrorCode::Unavailable
                    },
                    None,
                    key,
                )
            })?;
            decode_logged(&decoding, started, &bytes, kind, side)
        })
        .await
        .map_err(|_| {
            failed(
                &diagnostic,
                started,
                "join",
                ErrorCode::Internal,
                None,
                "chat_image_unavailable",
            )
        })?;
    }
    let request = client.prepare(source.command().expect("published image download"));
    let request = match &source {
        ImageSource::File {
            context: Some(context),
            ..
        } => request.with_plugin(context.clone()),
        _ => request,
    };
    diagnostic.context.request = Some(request.id);
    let output = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(failed(&diagnostic, started, "command", ErrorCode::Cancelled, None, "chat_image_unavailable")),
        output = client.execute(request) => output.map_err(|fault| failed(&diagnostic, started, "command", fault.code, None, "chat_image_unavailable"))?,
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
                let oversized = download.size > sailry_protocol::attachment::MAX_BYTES;
                return Err(failed(
                    &diagnostic,
                    started,
                    if oversized { "size" } else { "output_identity" },
                    ErrorCode::InvalidRequest,
                    Some(download.size),
                    if oversized {
                        "chat_image_too_large"
                    } else {
                        "chat_image_unavailable"
                    },
                ));
            }
            client
                .download(&download, &mut bytes, cancel.clone(), |_| {})
                .await
                .map_err(|fault| {
                    failed(
                        &diagnostic,
                        started,
                        "transfer",
                        fault.code,
                        Some(bytes.len() as u64),
                        "chat_image_unavailable",
                    )
                })?;
        }
        (_, Output::AttachmentDownload(download)) => {
            if source.attachment() != Some(&download.attachment) {
                let _ = client
                    .execute(client.prepare(Command::CancelFileTransfer {
                        stream: download.stream,
                    }))
                    .await;
                return Err(failed(
                    &diagnostic,
                    started,
                    "output_identity",
                    ErrorCode::InvalidRequest,
                    None,
                    "chat_image_unavailable",
                ));
            }
            client
                .download_attachment(&download, &mut bytes, cancel.clone(), |_| {})
                .await
                .map_err(|fault| {
                    failed(
                        &diagnostic,
                        started,
                        "transfer",
                        fault.code,
                        Some(bytes.len() as u64),
                        "chat_image_unavailable",
                    )
                })?;
        }
        _ => {
            return Err(failed(
                &diagnostic,
                started,
                "output_type",
                ErrorCode::InvalidRequest,
                None,
                "chat_image_unavailable",
            ));
        }
    }
    let decoding = diagnostic.clone();
    let length = bytes.len() as u64;
    tokio::task::spawn_blocking(move || {
        // Keep the slot until decoding really finishes, including after cancellation.
        let _permit = permit;
        if cancel.is_cancelled() {
            return Err(failed(
                &decoding,
                started,
                "decode",
                ErrorCode::Cancelled,
                Some(length),
                "chat_image_unavailable",
            ));
        }
        decode_logged(&decoding, started, &bytes, kind, side)
    })
    .await
    .map_err(|_| {
        failed(
            &diagnostic,
            started,
            "join",
            ErrorCode::Internal,
            Some(length),
            "chat_image_unavailable",
        )
    })?
}

fn failed(
    entry: &Entry,
    started: Instant,
    phase: &'static str,
    code: ErrorCode,
    bytes: Option<u64>,
    key: &'static str,
) -> &'static str {
    failure(entry, started, phase, code, bytes).emit();
    key
}

fn failure(
    entry: &Entry,
    started: Instant,
    phase: &'static str,
    code: ErrorCode,
    bytes: Option<u64>,
) -> Entry {
    let mut entry = entry.clone();
    entry.phase = if code == ErrorCode::Cancelled {
        Phase::Cancelled
    } else {
        Phase::Failed
    };
    entry.reason = Some(phase);
    entry.code = Some(code);
    entry.bytes = bytes;
    entry.duration_us = Some(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    entry
}

fn decode_logged(
    entry: &Entry,
    started: Instant,
    bytes: &[u8],
    kind: source::Kind,
    side: u32,
) -> Result<Arc<RenderImage>, &'static str> {
    let result = decode_kind(bytes, kind, side).map_err(|key| {
        failed(
            entry,
            started,
            "decode",
            ErrorCode::InvalidRequest,
            Some(bytes.len() as u64),
            key,
        )
    })?;
    let mut entry = entry.clone();
    entry.phase = Phase::Completed;
    entry.reason = Some("decode");
    entry.bytes = Some(bytes.len() as u64);
    entry.duration_us = Some(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    entry.emit();
    Ok(result)
}

fn decode_kind(
    bytes: &[u8],
    kind: source::Kind,
    side: u32,
) -> Result<Arc<RenderImage>, &'static str> {
    match kind {
        source::Kind::Raster(format) => decode::decode(bytes, format, side),
        source::Kind::Svg => decode::svg(bytes, side),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn distinguishes_failure_phases() {
        let mut entry = Entry::new(Area::Image, Phase::Begin);
        entry.source_kind = Some("file");
        let worktree = sailry_protocol::WorktreeId::new();
        entry.context.worktree = Some(worktree);
        for (reason, code) in [
            ("command", ErrorCode::NotFound),
            ("transfer", ErrorCode::Unavailable),
            ("decode", ErrorCode::InvalidRequest),
            ("slot", ErrorCode::Cancelled),
        ] {
            let observed = failure(&entry, Instant::now(), reason, code, Some(512));
            assert_eq!(observed.reason, Some(reason));
            assert_eq!(observed.code, Some(code));
            assert_eq!(observed.context.worktree, Some(worktree));
            assert_eq!(observed.source_kind, Some("file"));
            assert_eq!(observed.bytes, Some(512));
            assert_eq!(
                matches!(observed.phase, Phase::Cancelled),
                code == ErrorCode::Cancelled
            );
            assert!(observed.duration_us.is_some());
        }
    }

    #[test]
    fn preserves_image_error_keys() {
        let entry = Entry::new(Area::Image, Phase::Begin);
        assert_eq!(
            failed(
                &entry,
                Instant::now(),
                "size",
                ErrorCode::InvalidRequest,
                None,
                "chat_image_too_large"
            ),
            "chat_image_too_large"
        );
        assert_eq!(
            decode_logged(
                &entry,
                Instant::now(),
                b"not a PNG",
                source::Kind::Raster(image::ImageFormat::Png),
                128
            )
            .err(),
            Some("chat_image_unavailable")
        );
    }
}
