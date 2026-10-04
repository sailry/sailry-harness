//! File previews use a native WebView. Kit b79f4ce's WebView sets unclipped native
//! bounds and only hides on entity destruction, so a small mount adapter below
//! applies the GPUI content mask and releases visibility with the painted frame.
use super::*;
use raw_window_handle::HasWindowHandle;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output};
use std::{cell::Cell, rc::Rc};
#[path = "preview/documents.rs"]
mod documents;
#[cfg(target_os = "macos")]
#[path = "preview/pointer.rs"]
mod pointer;
#[path = "preview/surface.rs"]
mod surface;
#[cfg(test)]
#[path = "preview/tests.rs"]
mod tests;

struct Html {
    result: Option<Result<Rc<str>, &'static str>>,
    stop: CancellationToken,
    _load: Task<()>,
}
impl Drop for Html {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl Html {
    fn new(
        binding: Binding,
        file: File,
        context: Option<sailry_protocol::plugin::Context>,
        cx: &mut Context<Self>,
    ) -> Self {
        let stop = CancellationToken::new();
        let cancel = stop.clone();
        let job = binding.runtime.spawn(async move {
            load(binding.client, binding.worktree, file, context, cancel).await
        });
        let task = cx.spawn(async move |view, cx| {
            let result = job.await.unwrap_or(Err("artifact_preview_failed"));
            let _ = view.update(cx, |view, cx| {
                view.result = Some(result.map(Rc::from));
                cx.notify();
            });
        });
        Self {
            result: None,
            stop,
            _load: task,
        }
    }
}
impl Render for Html {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(Ok(html)) = &self.result {
            let html = html.clone();
            let native = window
                .use_keyed_state("html-native", cx, move |window, _| {
                    surface::Native::new(&html, window).map(Rc::new)
                })
                .read(cx)
                .clone();
            if let Ok(native) = native {
                return surface::render(native, window, cx);
            }
        }
        div()
            .size_full()
            .p_4()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(tr(match self.result {
                None => "artifact_preview_loading",
                Some(Err(key)) => key,
                _ => "artifact_preview_failed",
            }))
            .into_any_element()
    }
}

async fn load(
    client: std::sync::Arc<sailry_client::Client>,
    worktree: Option<WorktreeId>,
    file: File,
    context: Option<sailry_protocol::plugin::Context>,
    stop: CancellationToken,
) -> Result<String, &'static str> {
    let bytes = read(client, worktree, file, context, stop, 2 * 1024 * 1024).await?;
    String::from_utf8(bytes).map_err(|_| "artifact_preview_failed")
}

async fn read(
    client: std::sync::Arc<sailry_client::Client>,
    worktree: Option<WorktreeId>,
    file: File,
    context: Option<sailry_protocol::plugin::Context>,
    stop: CancellationToken,
    limit: u64,
) -> Result<Vec<u8>, &'static str> {
    let worktree = worktree.ok_or("artifact_preview_failed")?;
    let mut request = client.prepare(Command::DownloadFile {
        worktree,
        path: file.path.clone(),
    });
    if let Some(context) = context {
        request = request.with_plugin(context);
    }
    let Output::FileDownload(download) = client
        .execute(request)
        .await
        .map_err(|_| "artifact_preview_failed")?
    else {
        return Err("artifact_preview_failed");
    };
    let invalid = if download.worktree != worktree || download.path != file.path {
        Some("artifact_preview_failed")
    } else if download.size > limit {
        Some("artifact_preview_large")
    } else if file
        .revision
        .as_ref()
        .is_some_and(|revision| revision != &download.revision)
    {
        Some("artifact_preview_changed")
    } else {
        None
    };
    if let Some(key) = invalid {
        let _ = client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: download.stream,
            }))
            .await;
        return Err(key);
    }
    let mut bytes = Vec::new();
    client
        .download(&download, &mut bytes, stop, |_| {})
        .await
        .map_err(|_| "artifact_preview_failed")?;
    Ok(bytes)
}

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
#[path = "preview/native_check.rs"]
pub(super) mod check;

pub(super) fn create(
    binding: Binding,
    file: File,
    source: WeakEntity<Chat>,
    context: Option<sailry_protocol::plugin::Context>,
    cx: &mut App,
) -> Option<AnyView> {
    if file.mime == "text/html" {
        Some(cx.new(|cx| Html::new(binding, file, context, cx)).into())
    } else {
        documents::create(binding, file, source, context, cx)
    }
}
