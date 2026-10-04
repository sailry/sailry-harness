//! Office rendering stays in the existing side-panel WebView. Kit b79f4ce has no
//! PDF renderer; its native controls still own navigation and zoom.
//! The execution Node converts Office files to PDF before this viewer mounts.
use super::*;
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;

#[path = "documents/assets.rs"]
pub(super) mod assets;
#[cfg(test)]
#[path = "documents/tests.rs"]
mod tests;

const PDF: &str = "application/pdf";
const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Default, Deserialize)]
struct Status {
    #[serde(default)]
    pages: u32,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    scale: f32,
    error: Option<String>,
    #[serde(default)]
    selection: String,
}

struct Document {
    source: WeakEntity<Chat>,
    binding: Binding,
    file: File,
    context: Option<sailry_protocol::plugin::Context>,
    bytes: Option<Arc<[u8]>>,
    native: Option<Rc<surface::Native>>,
    error: Option<&'static str>,
    status: Status,
    background: String,
    warnings: Vec<String>,
    stop: CancellationToken,
    _load: Task<()>,
    _events: Option<Task<()>>,
}

impl Drop for Document {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Document {
    fn new(
        binding: Binding,
        file: File,
        source: WeakEntity<Chat>,
        context: Option<sailry_protocol::plugin::Context>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            source,
            binding,
            file,
            context,
            bytes: None,
            native: None,
            error: None,
            status: Status::default(),
            background: String::new(),
            warnings: Vec::new(),
            stop: CancellationToken::new(),
            _load: Task::ready(()),
            _events: None,
        };
        view.load(cx);
        view
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.native = None;
        self.bytes = None;
        self._events = None;
        self.error = None;
        self.warnings.clear();
        self.status = Status::default();
        let binding = self.binding.clone();
        let file = self.file.clone();
        let context = self.context.clone();
        let stop = self.stop.clone();
        let job = self.binding.runtime.spawn(async move {
            let (bytes, warnings) = if file.mime == PDF {
                (
                    read(
                        binding.client,
                        binding.worktree,
                        file.clone(),
                        context,
                        stop,
                        LIMIT,
                    )
                    .await?,
                    Vec::new(),
                )
            } else {
                converted(binding.client, binding.worktree, file, context, stop).await?
            };
            tokio::task::spawn_blocking(move || {
                assets::validate(PDF, &bytes)?;
                assets::bundle()?;
                Ok::<_, &'static str>((Arc::from(bytes), warnings))
            })
            .await
            .unwrap_or(Err("artifact_preview_failed"))
        });
        self._load = cx.spawn(async move |view, cx| {
            let result = job.await.unwrap_or(Err("artifact_preview_failed"));
            let _ = view.update(cx, |view, cx| {
                match result {
                    Ok((bytes, warnings)) => {
                        view.bytes = Some(bytes);
                        view.warnings = warnings;
                    }
                    Err(key) => view.error = Some(key),
                }
                cx.notify();
            });
        });
    }

    fn mount(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(bytes) = self.bytes.take() else {
            return;
        };
        let (send, mut receive) = futures::channel::mpsc::unbounded();
        match assets::native(
            bytes,
            PDF,
            &self.background,
            move |body| {
                if let Ok(status) = serde_json::from_str::<Status>(body) {
                    let _ = send.unbounded_send(status);
                }
            },
            window,
        ) {
            Ok(native) => self.native = Some(Rc::new(native)),
            Err(()) => self.error = Some("artifact_preview_failed"),
        }
        self._events = Some(cx.spawn(async move |view, cx| {
            while let Some(status) = receive.next().await {
                if view
                    .update(cx, |view, cx| {
                        if let Some(error) = status.error.as_deref() {
                            view.error = Some(match error {
                                "password" => "artifact_preview_password",
                                _ => "artifact_preview_failed",
                            });
                        }
                        view.status = status;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn action(&mut self, action: &str, cx: &mut Context<Self>) {
        if self.native.as_ref().is_some_and(|native| {
            native
                .page
                .evaluate_script(&format!("window.previewAction?.('{action}')"))
                .is_err()
        }) {
            self.error = Some("artifact_preview_failed");
            cx.notify();
        }
    }
}

impl Render for Document {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = cx.theme().muted.to_rgb();
        let background = format!(
            "rgb({:.0} {:.0} {:.0})",
            color.r * 255.,
            color.g * 255.,
            color.b * 255.
        );
        if self.background != background {
            self.background = background;
            if let Some(native) = &self.native {
                let _ = native.page.evaluate_script(&format!(
                    "window.previewTheme?.({})",
                    serde_json::to_string(&self.background).unwrap()
                ));
            }
        }
        self.mount(window, cx);
        let ready = self.status.pages > 0 && self.error.is_none();
        let mut toolbar = h_flex()
            .px_2()
            .py_1()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border);
        for (id, icon, label, disabled) in [
            (
                "previous",
                IconName::ChevronLeft,
                "artifact_previous_page",
                !ready || self.status.page <= 1,
            ),
            (
                "next",
                IconName::ChevronRight,
                "artifact_next_page",
                !ready || self.status.page >= self.status.pages,
            ),
        ] {
            toolbar = toolbar.child(
                Button::new(id)
                    .ghost()
                    .small()
                    .icon(icon)
                    .tooltip(tr(label))
                    .disabled(disabled)
                    .on_click(cx.listener(move |view, _, _, cx| view.action(id, cx))),
            );
        }
        toolbar = toolbar
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if ready {
                        format!("{} / {}", self.status.page, self.status.pages)
                    } else {
                        String::new()
                    }),
            )
            .child(div().flex_1());
        for (id, icon, label) in [
            ("out", IconName::Minus, "artifact_zoom_out"),
            ("in", IconName::Plus, "artifact_zoom_in"),
        ] {
            toolbar = toolbar.child(
                Button::new(id)
                    .ghost()
                    .small()
                    .icon(icon)
                    .tooltip(tr(label))
                    .disabled(!ready)
                    .on_click(cx.listener(move |view, _, _, cx| view.action(id, cx))),
            );
        }
        toolbar = toolbar
            .child(
                Button::new("request-edit")
                    .ghost()
                    .small()
                    .label(tr("artifact_request_edit"))
                    .disabled(
                        !ready
                            || !self
                                .source
                                .read_with(cx, |source, _| {
                                    source.can_request_file_edit(self.binding.worktree)
                                })
                                .unwrap_or(false),
                    )
                    .on_click(cx.listener(|view, _, window, cx| {
                        let _ = view.source.update(cx, |source, cx| {
                            source.request_file_edit(
                                view.binding.worktree,
                                view.file.path.clone(),
                                view.status.page,
                                &view.status.selection,
                                window,
                                cx,
                            )
                        });
                    })),
            )
            .child(
                Button::new("fit")
                    .ghost()
                    .small()
                    .label(if ready {
                        format!("{:.0}%", self.status.scale * 100.)
                    } else {
                        tr("artifact_fit_width").to_string()
                    })
                    .tooltip(tr("artifact_fit_width"))
                    .disabled(!ready)
                    .on_click(cx.listener(|view, _, _, cx| view.action("fit", cx))),
            )
            .child(
                Button::new("reload")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .tooltip(tr("artifact_reload"))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.file.revision = None;
                        view.load(cx);
                        cx.notify();
                    })),
            );
        if !self.warnings.is_empty() {
            toolbar = toolbar.child(
                Button::new("compatibility")
                    .ghost()
                    .small()
                    .icon(IconName::Info)
                    .accessibility_label(tr("artifact_preview_warnings"))
                    .tooltip(self.warnings.join("\n")),
            );
        }
        let content = if let Some(key) = self.error {
            div()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr(key))
                .into_any_element()
        } else if let Some(native) = &self.native {
            surface::render(native.clone(), window, cx)
        } else {
            div()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(tr("artifact_preview_loading"))
                .into_any_element()
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(toolbar)
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
    }
}

async fn converted(
    client: Arc<sailry_client::Client>,
    worktree: Option<WorktreeId>,
    file: File,
    context: Option<sailry_protocol::plugin::Context>,
    stop: CancellationToken,
) -> Result<(Vec<u8>, Vec<String>), &'static str> {
    let worktree = worktree.ok_or("artifact_preview_failed")?;
    let mut request = client.prepare(Command::PreviewOffice {
        worktree,
        path: file.path.clone(),
    });
    if let Some(context) = context {
        request = request.with_plugin(context);
    }
    let response = tokio::select! {
        _ = stop.cancelled() => return Err("artifact_preview_failed"),
        response = client.execute(request) => response.map_err(|_| "artifact_preview_failed")?,
    };
    let Output::OfficePreview(preview) = response else {
        return Err("artifact_preview_failed");
    };
    let error = if preview.download.size > LIMIT {
        Some("artifact_preview_large")
    } else if file
        .revision
        .as_ref()
        .is_some_and(|r| *r != preview.source_revision)
    {
        Some("artifact_preview_changed")
    } else {
        None
    };
    if let Some(error) = error {
        let _ = client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: preview.download.stream,
            }))
            .await;
        return Err(error);
    }
    let mut bytes = Vec::new();
    client
        .download(&preview.download, &mut bytes, stop, |_| {})
        .await
        .map_err(|_| "artifact_preview_failed")?;
    Ok((bytes, preview.warnings))
}

pub(super) fn create(
    binding: Binding,
    file: File,
    source: WeakEntity<Chat>,
    context: Option<sailry_protocol::plugin::Context>,
    cx: &mut App,
) -> Option<AnyView> {
    matches!(file.mime.as_str(), PDF | DOCX | XLSX | PPTX).then(|| {
        cx.new(|cx| Document::new(binding, file, source, context, cx))
            .into()
    })
}
