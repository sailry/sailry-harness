//! Conversation inventory composition; Node owns extraction and Client owns pagination.
use super::*;
use crate::content::images::{ImageSource, Images};
use crate::theme::DialogStyle as _;
use sailry_protocol::conversation::assets::{Group, Kind, Query, Target};
mod render;
#[cfg(test)]
mod tests;

pub(crate) struct Assets {
    owner: WeakEntity<View>,
    binding: Binding,
    session: Option<SessionId>,
    revision: Option<u64>,
    loaded_revision: Option<u64>,
    images: Entity<Images>,
    scroll: ScrollHandle,
    kind: Option<Kind>,
    groups: Vec<Group>,
    before: Option<u64>,
    loading: bool,
    failed: bool,
    retry_more: bool,
    open: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
}

impl Drop for Assets {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Assets {
    pub(super) fn new(
        owner: WeakEntity<View>,
        binding: Binding,
        session: Option<SessionId>,
        images: Entity<Images>,
    ) -> Self {
        Self {
            owner,
            binding,
            session,
            images,
            scroll: ScrollHandle::new(),
            revision: None,
            loaded_revision: None,
            kind: None,
            groups: Vec::new(),
            before: None,
            loading: false,
            failed: false,
            retry_more: false,
            open: false,
            stop: CancellationToken::new(),
            task: None,
        }
    }

    pub(super) fn sync(
        &mut self,
        binding: &Binding,
        images: &Entity<Images>,
        session: Option<SessionId>,
        revision: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.images = images.clone();
        if self.session == session
            && self.revision == revision
            && self.binding.client.target() == binding.client.target()
        {
            return;
        }
        self.stop.cancel();
        self.binding = binding.clone();
        self.session = session;
        self.revision = revision;
        self.groups.clear();
        self.before = None;
        self.loading = false;
        self.failed = false;
        if self.open {
            self.load(false, window, cx);
        }
        cx.notify();
    }

    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() {
            return;
        }
        self.open = true;
        self.load(false, window, cx);
        let panel = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let close = panel.clone();
            dialog
                .form_title(tr("chat_assets_title"))
                .w((window.viewport_size().width - px(48.)).min(px(720.)))
                .on_close(move |_, _, cx| {
                    close.update(cx, |panel, cx| {
                        panel.open = false;
                        panel.stop.cancel();
                        panel.loading = false;
                        cx.notify();
                    })
                })
                .child(panel.update(cx, |panel, cx| panel.content(window, cx)))
        });
        cx.notify();
    }

    fn load(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.stop.cancel();
        self.task = None;
        let Some(session) = self.session else {
            return;
        };
        let before = if more { self.before } else { None };
        self.loading = true;
        self.failed = false;
        self.retry_more = more;
        self.stop = CancellationToken::new();
        let stop = self.stop.clone();
        let cancel = stop.clone();
        let client = self.binding.client.clone();
        let query = Query {
            kind: self.kind,
            before,
            limit: 20,
        };
        let job = self.binding.runtime.spawn(async move {
            tokio::select! { _ = cancel.cancelled() => None, value = client.conversation_assets(session, query) => Some(value) }
        });
        self.task = Some(cx.spawn_in(window, async move |panel, cx| {
            let result = job.await;
            if stop.is_cancelled() {
                return;
            }
            let _ = panel.update_in(cx, |panel, window, cx| {
                if stop.is_cancelled() {
                    return;
                }
                panel.loading = false;
                match result {
                    Ok(Some(Ok(page)))
                        if panel
                            .revision
                            .is_none_or(|revision| revision == page.revision)
                            && (!more
                                || panel
                                    .loaded_revision
                                    .is_none_or(|revision| revision == page.revision)) =>
                    {
                        if !more {
                            panel.groups.clear();
                        }
                        panel.loaded_revision = Some(page.revision);
                        panel.groups.extend(page.groups);
                        panel.before = page.next_before;
                    }
                    _ => {
                        panel.failed = true;
                        crate::feedback::error("", &tr("chat_assets_failed"), window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select(
        &mut self,
        target: Target,
        worktree: WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session else {
            return;
        };
        let source = match target {
            Target::Attachment(attachment) => ImageSource::Attachment(attachment),
            Target::Image(image) => ImageSource::Image { session, image },
            Target::File { path } => {
                if !crate::content::images::supports(&path) {
                    self.open = false;
                    self.stop.cancel();
                    window.close_dialog(cx);
                    let _ = self
                        .owner
                        .update(cx, |_, cx| cx.emit(Event::FileAt(worktree, path)));
                    return;
                }
                ImageSource::File {
                    worktree,
                    path,
                    context: None,
                }
            }
        };
        if source.format().is_some() {
            Images::open(&self.images, source, window, cx);
        } else {
            source.download(
                self.binding.client.clone(),
                self.binding.runtime.clone(),
                window,
                cx,
            );
        }
    }
}
