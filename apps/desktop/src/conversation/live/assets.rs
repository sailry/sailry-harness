//! Conversation inventory composition; Node owns extraction and Client owns pagination.
use super::*;
use crate::content::images::{ImageSource, Images};
use sailry_protocol::conversation::assets::{Group, Kind, Query, Target};
mod cache;
mod overview;
mod render;
use cache::{Cache, Loaded};

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
    loaded: Option<Loaded>,
    cache: Cache,
    images: Entity<Images>,
    scroll: ScrollHandle,
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
    pub(super) fn contains_document(
        &self,
        target: NodeId,
        session: SessionId,
        revision: u64,
        document: &sailry_protocol::artifact::Summary,
    ) -> bool {
        let listed = |group: &Group| {
            group.kind == Kind::Artifact
                && group.turn == document.turn
                && group
                    .items
                    .iter()
                    .any(|target| matches!(target, Target::Document(value) if value == document))
        };
        document.valid()
            && self.binding.client.target() == target
            && self.session == Some(session)
            && self.loaded.as_ref().is_some_and(|loaded| {
                loaded.revision == revision && loaded.groups.iter().any(listed)
            })
    }

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
            loaded: None,
            cache: Cache::default(),
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
        let target = binding.client.target();
        let previous = self.binding.client.target();
        if self.session == session && self.stamp == stamp && previous == target {
            return;
        }
        self.stop.cancel();
        self.task = None;
        self.loading = false;
        self.failed = false;
        if self.session != session || previous != target {
            if let (Some(id), Some(loaded)) = (self.session, self.loaded.take()) {
                self.cache.put(previous, id, loaded);
            }
            self.loaded = session.and_then(|id| self.cache.take(target, id));
        }
        self.binding = binding.clone();
        self.session = session;
        self.stamp = stamp;
        if self
            .loaded
            .as_ref()
            .is_some_and(|loaded| !loaded.reusable(stamp))
        {
            self.loaded = None;
        }
        if self.open && !self.current() {
            self.load(false, window, cx);
        }
        cx.notify();
    }

    /// Whether the shown inventory already covers the owner's newest history.
    fn current(&self) -> bool {
        self.loaded
            .as_ref()
            .is_some_and(|loaded| loaded.current(self.stamp))
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open == open || open && self.session.is_none() {
            return;
        }
        self.open = open;
        if open {
            // A cached inventory renders at once; only newer history triggers a refresh.
            if !self.current() {
                self.load(false, window, cx);
            }
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
        let before = if more {
            self.loaded.as_ref().and_then(|loaded| loaded.before)
        } else {
            None
        };
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
                                    .loaded
                                    .as_ref()
                                    .is_none_or(|loaded| loaded.revision == page.revision)) =>
                    {
                        let mut groups = match panel.loaded.take() {
                            Some(loaded) if more => loaded.groups,
                            _ => Vec::new(),
                        };
                        groups.extend(page.groups);
                        panel.loaded = Some(Loaded {
                            revision: page.revision,
                            head,
                            groups,
                            before: page.next_before,
                        });
                    }
                    _ => {
                        panel.failed = true;
                        crate::feedback::error(
                            "chat_assets_failed",
                            "",
                            &tr("chat_assets_failed"),
                            window,
                            cx,
                        );
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
            Target::Document(document) => {
                self.set_open(false, window, cx);
                let _ = self
                    .owner
                    .update(cx, |_, cx| cx.emit(Event::Document(document)));
                return;
            }
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
        if source.kind().is_some() {
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
        let _ = self.owner.update(cx, |_, cx| {
            cx.emit(Event::Link(
                None,
                uri.into(),
                crate::conversation::links::Mode::Internal,
            ))
        });
    }
}
