//! Composer-owned drafts. Published bytes and admission remain owned by the execution Node.
use super::*;
use gpui_kit::InputEvent as _;
use sailry_protocol::{AttachmentId, attachment::Attachment};
use std::path::PathBuf;

pub(super) use crate::content::images;
mod render;
mod source;
pub(super) use render::{image_links, links, source_links};
use source::Source;
#[cfg(test)]
#[path = "tests/fixture/images.rs"]
pub(crate) mod image_testing;
#[cfg(test)]
pub(in crate::conversation::live) mod tests;
mod transfer;
use transfer::Status;

pub(super) fn pointer_drag(
    event: &DragMoveEvent<ExternalPaths>,
    window: &mut Window,
    cx: &mut App,
) {
    // GPUI-pre 0.3.4 translates FileDrop after tracking input modality, leaving
    // keyboard hover suppression active. Re-enter its pointer path after dispatch.
    if window.last_input_was_keyboard() {
        let event = event.event.clone();
        window.defer(cx, move |window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
    }
}

pub(super) struct Drafts {
    next: usize,
    items: Vec<Item>,
    pub(super) error: Option<&'static str>,
    pub(super) slots: Arc<tokio::sync::Semaphore>,
}

impl Default for Drafts {
    fn default() -> Self {
        Self {
            next: 0,
            items: Vec::new(),
            error: None,
            slots: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}

struct Item {
    key: usize,
    source: Source,
    name: String,
    status: Status,
    pending: bool,
    removing: bool,
    cancel: CancellationToken,
}

impl Drop for Item {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Drafts {
    pub(super) fn can_retarget(&self) -> bool {
        self.items.iter().all(|item| {
            !item.pending
                && !item.removing
                && matches!(
                    item.status,
                    Status::Staged | Status::Ready(_) | Status::Failed(_)
                )
        })
    }

    pub(super) fn sendable(&self) -> bool {
        self.items.iter().all(|item| {
            !item.pending
                && !item.removing
                && matches!(item.status, Status::Staged | Status::Ready(_))
        })
    }
    pub(super) fn ready(&self) -> bool {
        self.items
            .iter()
            .all(|item| !item.pending && !item.removing && matches!(item.status, Status::Ready(_)))
    }

    pub(super) fn ids(&self) -> Vec<AttachmentId> {
        self.items
            .iter()
            .filter_map(|item| match &item.status {
                Status::Ready(attachment) => Some(attachment.id),
                _ => None,
            })
            .collect()
    }

    pub(super) fn sent(&mut self, ids: &[AttachmentId]) {
        self.items.retain(|item| !matches!(&item.status, Status::Ready(attachment) if ids.contains(&attachment.id)));
    }
}

impl View {
    pub(super) fn restore_attachment_sources(
        &mut self,
        sources: Vec<Source>,
        cx: &mut Context<Self>,
    ) {
        // Recovery stages local bytes only. Unlike a manual attachment pick in an
        // existing session, it must not upload or admit a durable command.
        for source in sources {
            let Some(name) = source.name() else {
                continue;
            };
            let key = self.attachments.next;
            self.attachments.next += 1;
            self.attachments.items.push(Item {
                key,
                source,
                name,
                status: Status::Staged,
                pending: false,
                removing: false,
                cancel: self.stop.child_token(),
            });
        }
        cx.notify();
    }
    pub(super) fn attachments_blocked(&self) -> bool {
        self.readonly() || self.busy() || (self.session.is_some() && !self.connected())
    }

    pub(super) fn has_attachments(&self) -> bool {
        !self.attachments.items.is_empty()
    }

    pub(super) fn attachment_sources(&self) -> Vec<Source> {
        self.attachments
            .items
            .iter()
            .map(|item| item.source.clone())
            .collect()
    }

    pub(super) fn set_attachment_sequence(&mut self, next: usize) {
        self.attachments.next = next;
    }

    pub(super) fn release_draft_attachments(&mut self) -> usize {
        for item in &self.attachments.items {
            if let Status::Ready(attachment) = &item.status {
                let client = self.binding.client.clone();
                let pending = transfer::discard(&client, attachment);
                self.binding.runtime.spawn(async move {
                    if !matches!(transfer::check(client, pending).await, Status::Removed) {
                        eprintln!("Draft attachment cleanup did not complete");
                    }
                });
            }
        }
        self.attachments.next
    }

    pub(super) fn attachment_previews(&self) -> Vec<images::ImageSource> {
        self.attachments
            .items
            .iter()
            .map(|item| match &item.status {
                Status::Ready(attachment) => images::ImageSource::from(attachment),
                _ => images::ImageSource::Local {
                    key: item.key,
                    source: item.source.clone(),
                },
            })
            .collect()
    }

    pub(super) fn prepare_attachments(
        &mut self,
        message: sailry_protocol::conversation::Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let keys: Vec<_> = self
            .attachments
            .items
            .iter()
            .filter(|item| matches!(item.status, Status::Staged))
            .map(|item| item.key)
            .collect();
        if keys.is_empty() {
            return false;
        }
        self.preparing = Some(message);
        for key in keys {
            self.upload_attachment(key, window, cx);
        }
        cx.notify();
        true
    }

    pub(super) fn choose_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachments_blocked() {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(tr("composer_attachment")),
        });
        cx.spawn_in(window, async move |view, cx| {
            let selected = selected.await;
            let _ = view.update_in(cx, |view, window, cx| match selected {
                Ok(Ok(Some(paths))) => view.attach_paths(paths, window, cx),
                Ok(Ok(None)) => {}
                _ => {
                    view.attachments.error = Some("files_upload_picker");
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(in crate::conversation::live) fn attach_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.attach_sources(paths.into_iter().map(Source::Path).collect(), window, cx);
    }

    pub(super) fn paste_attachments(
        &mut self,
        _: &gpui_kit::component::input::Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(clipboard) = cx.read_from_clipboard() else {
            return;
        };
        let mut sources = Vec::new();
        for entry in clipboard.into_entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => {
                    sources.extend(paths.paths().iter().cloned().map(Source::Path))
                }
                ClipboardEntry::Image(image) => {
                    let name = format!(
                        "{}.{}",
                        rust_i18n::t!(
                            "chat_clipboard_image",
                            count = self.attachments.next + sources.len() + 1
                        ),
                        if matches!(image.format, ImageFormat::Tiff | ImageFormat::Bmp) {
                            "png"
                        } else {
                            image.format.extension()
                        }
                    );
                    sources.push(Source::Image {
                        name,
                        format: image.format,
                        bytes: image.bytes.into(),
                    });
                }
                ClipboardEntry::String(_) => {}
            }
        }
        if !sources.is_empty() {
            cx.stop_propagation();
            self.attach_sources(sources, window, cx);
        }
    }

    pub(super) fn attach_sources(
        &mut self,
        sources: Vec<Source>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.attachments_blocked() {
            return;
        }
        if sources.len() > 8usize.saturating_sub(self.attachments.items.len()) {
            self.attachments.error = Some("chat_attachment_limit");
            cx.notify();
            return;
        }
        self.attachments.error = None;
        for source in sources {
            let Some(name) = source.name() else {
                self.attachments.error = Some("files_upload_name");
                continue;
            };
            let key = self.attachments.next;
            self.attachments.next += 1;
            self.attachments.items.push(Item {
                key,
                source,
                name,
                status: if self.session.is_none() {
                    Status::Staged
                } else {
                    Status::Preparing
                },
                pending: false,
                removing: false,
                cancel: self.stop.child_token(),
            });
            if self.session.is_some() {
                self.upload_attachment(key, window, cx);
            }
        }
        cx.notify();
    }

    fn upload_attachment(&mut self, key: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Creation already returned an authoritative workspace. Its history or
        // snapshot subscription can still be connecting; transfer failures use
        // the existing retry state instead of leaving staged files idle.
        if self.readonly() || self.pending || self.retry.is_some() {
            return;
        }
        let Some(worktree) = self.binding.worktree else {
            return;
        };
        let Some(item) = self
            .attachments
            .items
            .iter_mut()
            .find(|item| item.key == key)
        else {
            return;
        };
        if item.pending {
            return;
        }
        let previous = item.status.clone();
        let source = item.source.clone();
        let client = self.binding.client.clone();
        let slots = self.attachments.slots.clone();
        item.cancel = self.stop.child_token();
        let cancel = item.cancel.clone();
        item.pending = true;
        item.status = match &previous {
            Status::Uncertain(pending) | Status::Rejected(pending) => {
                Status::Publishing(pending.clone())
            }
            _ => Status::Preparing,
        };
        let mut last = item.status.clone();
        let (updates, mut changes) = tokio::sync::watch::channel(item.status.clone());
        let job = self.binding.runtime.spawn(async move {
            match previous {
                Status::Uncertain(pending) | Status::Rejected(pending) => {
                    transfer::check(client, *pending).await
                }
                _ => transfer::run(client, worktree, source, cancel, updates, slots).await,
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            while changes.changed().await.is_ok() {
                let status = changes.borrow_and_update().clone();
                last = status.clone();
                let _ = view.update_in(cx, |view, _, cx| {
                    if let Some(item) = view
                        .attachments
                        .items
                        .iter_mut()
                        .find(|item| item.key == key)
                    {
                        item.status = status;
                        cx.notify();
                    }
                });
            }
            let status = job.await.unwrap_or_else(|_| match last {
                Status::Publishing(pending) => Status::Uncertain(pending),
                _ => Status::Failed("files_upload_failed"),
            });
            let _ = view.update_in(cx, |view, window, cx| {
                view.finish_attachment(key, status, window, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_attachment(
        &mut self,
        key: usize,
        status: Status,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .attachments
            .items
            .iter_mut()
            .find(|item| item.key == key)
        else {
            return;
        };
        item.pending = false;
        item.status = status;
        if matches!(item.status, Status::Removed)
            || item.removing && matches!(item.status, Status::Failed(_))
        {
            self.attachments.items.retain(|item| item.key != key);
            if self.error == Some("chat_attachment_rejected") {
                self.error = None;
            }
        } else if item.removing && matches!(item.status, Status::Ready(_)) {
            self.discard_attachment(key, window, cx);
        }
        if self.preparing.is_some() && !self.attachments.items.iter().any(|item| item.pending) {
            let mut message = self.preparing.take().unwrap();
            if self.attachments.ready() {
                message.attachments = self.attachments.ids();
                self.submit(message, window, cx);
            } else {
                self.outgoing = None;
                self.sync_rows(cx);
            }
        }
        cx.notify();
    }

    fn remove_attachment(&mut self, key: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachments_blocked() {
            return;
        }
        let Some(item) = self
            .attachments
            .items
            .iter_mut()
            .find(|item| item.key == key)
        else {
            return;
        };
        match &item.status {
            Status::Preparing | Status::Sending { .. } => {
                item.removing = true;
                item.cancel.cancel();
            }
            Status::Ready(_) => {
                self.discard_attachment(key, window, cx);
            }
            Status::Staged | Status::Failed(_) => {
                self.attachments.items.retain(|item| item.key != key)
            }
            _ => return,
        }
        cx.notify();
    }

    fn discard_attachment(&mut self, key: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .attachments
            .items
            .iter_mut()
            .find(|item| item.key == key)
        else {
            return;
        };
        let Status::Ready(attachment) = &item.status else {
            return;
        };
        let client = self.binding.client.clone();
        let pending = transfer::discard(&client, attachment);
        let uncertain = pending.clone();
        item.status = Status::Removing;
        item.removing = true;
        item.pending = true;
        let job = self.binding.runtime.spawn(transfer::check(client, pending));
        cx.spawn_in(window, async move |view, cx| {
            let status = job.await.unwrap_or(Status::Uncertain(Box::new(uncertain)));
            let _ = view.update_in(cx, |view, window, cx| {
                view.finish_attachment(key, status, window, cx)
            });
        })
        .detach();
    }
}
