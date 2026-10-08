//! Conversation inventory composition; Node owns extraction and Client owns pagination.
use super::*;
use crate::content::images::{ImageSource, Images};
use sailry_protocol::conversation::assets::{Group, Kind, Query, Target};
mod overview;
mod render;
#[cfg(test)]
mod tests;

impl View {
    pub(crate) fn open_file_image(
        &self,
        worktree: WorktreeId,
        path: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        Images::open_on_close(
            &self.images,
            ImageSource::File {
                worktree,
                path,
                context: None,
            },
            |_, _| {},
            window,
            cx,
        );
    }
}

pub(crate) struct Assets {
    owner: WeakEntity<View>,
    binding: Binding,
    session: Option<SessionId>,
    stamp: Option<(u64, u64)>,
    loaded_revision: Option<u64>,
    loaded_head: Option<u64>,
    images: Entity<Images>,
    scroll: ScrollHandle,
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
        cx: &mut Context<Self>,
    ) -> Self {
        if let Some(owner) = owner.upgrade() {
            cx.observe(&owner, |_, _, cx| cx.notify()).detach();
        }
        Self {
            owner,
            binding,
            session,
            images,
            scroll: ScrollHandle::new(),
            stamp: None,
            loaded_revision: None,
            loaded_head: None,
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
        stamp: Option<(u64, u64)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.images = images.clone();
        if self.session == session
            && self.stamp == stamp
            && self.binding.client.target() == binding.client.target()
        {
            return;
        }
        self.stop.cancel();
        self.binding = binding.clone();
        self.session = session;
        self.stamp = stamp;
        self.groups.clear();
        self.before = None;
        self.loaded_revision = None;
        self.loaded_head = None;
        self.loading = false;
        self.failed = false;
        if self.open {
            self.load(false, window, cx);
        }
        cx.notify();
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open || open && self.session.is_none() {
            return;
        }
        self.open = open;
        if open {
            self.load(false, window, cx);
        } else {
            self.stop.cancel();
            self.task = None;
            self.loading = false;
        }
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
        let head = self.stamp.map(|(_, head)| head);
        let query = Query {
            kind: None,
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
                            .stamp
                            .is_none_or(|(revision, _)| revision == page.revision)
                            && (!more
                                || panel
                                    .loaded_revision
                                    .is_none_or(|revision| revision == page.revision)) =>
                    {
                        if !more {
                            panel.groups.clear();
                        }
                        panel.loaded_revision = Some(page.revision);
                        panel.loaded_head = head;
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
        kind: Kind,
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
                    self.set_open(false, window, cx);
                    let event = if kind == Kind::Artifact {
                        Event::Artifact(
                            worktree,
                            sailry_protocol::tool::File {
                                mime: crate::content::files::mime(&path),
                                path,
                                name: None,
                                size: None,
                                revision: None,
                            },
                        )
                    } else {
                        Event::FileAt(worktree, path)
                    };
                    let _ = self.owner.update(cx, |_, cx| cx.emit(event));
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
            let panel = cx.entity().downgrade();
            let target = self.binding.client.target();
            let stamp = self.stamp;
            self.set_open(false, window, cx);
            Images::open_on_close(
                &self.images,
                source,
                move |_, cx| {
                    let _ = panel.update(cx, |panel, cx| {
                        if !panel.open
                            && panel.session == Some(session)
                            && panel.binding.client.target() == target
                            && panel.stamp == stamp
                        {
                            panel.open = true;
                            cx.notify();
                        }
                    });
                },
                window,
                cx,
            );
        } else {
            source.download(
                self.binding.client.clone(),
                self.binding.runtime.clone(),
                window,
                cx,
            );
        }
    }

    fn select_child(&mut self, id: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        self.set_open(false, window, cx);
        let _ = self.owner.update(cx, |view, cx| view.open_child(id, cx));
    }

    fn select_source(&mut self, uri: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_open(false, window, cx);
        let _ = self
            .owner
            .update(cx, |_, cx| cx.emit(Event::Link(uri.into())));
    }
}
