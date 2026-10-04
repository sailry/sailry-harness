//! View-owned, bounded thumbnails for local drafts and published Node attachments.
use crate::tr;
use gpui_kit::component::{
    attachment::{AttachmentActions, AttachmentMedia},
    button::Button,
};
use gpui_kit::{component::*, *};
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{AttachmentId, Command, Output, attachment::Attachment};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
struct Binding {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
}

const THUMBNAIL_SIDE: u32 = 320;

pub(crate) mod decode;
mod load;
mod preview;
mod render;
mod source;
pub(crate) use render::card;
pub(crate) use source::ImageSource;
mod local;
pub(crate) use local::Local;
#[cfg(test)]
mod tests;

pub(crate) struct Images {
    binding: Binding,
    slots: Arc<tokio::sync::Semaphore>,
    entries: HashMap<Key, Entry>,
    next: u64,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Local(usize),
    Published(AttachmentId),
    File(sailry_protocol::WorktreeId, String),
}

struct Entry {
    generation: u64,
    touched: u64,
    cancel: CancellationToken,
    result: Option<Result<Arc<RenderImage>, &'static str>>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Images {
    pub(crate) fn open(
        images: &Entity<Self>,
        source: ImageSource,
        window: &mut Window,
        cx: &mut App,
    ) {
        preview::open_gallery(images, vec![source], 0, window, cx);
    }

    pub(crate) fn new(
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        slots: Arc<tokio::sync::Semaphore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_release(|images, cx| {
            for (_, mut entry) in images.entries.drain() {
                if let Some(Ok(image)) = entry.result.take() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        Self {
            binding: Binding { client, runtime },
            slots,
            entries: HashMap::new(),
            next: 0,
        }
    }

    pub(crate) fn media(
        images: &Entity<Self>,
        source: impl Into<ImageSource>,
        cx: &App,
    ) -> AttachmentMedia {
        let source = source.into();
        if source.format().is_none()
            || images
                .read(cx)
                .entries
                .get(&source.key())
                .is_some_and(|entry| matches!(entry.result, Some(Err(_))))
        {
            return AttachmentMedia::new().child(Icon::new(IconName::File).size_4());
        }
        let images = images.downgrade();
        // Load through GPUI's image source callback, never during history reduction.
        AttachmentMedia::new().src(move |window: &mut Window, cx: &mut App| {
            images
                .update(cx, |images, cx| images.request(&source, window, cx))
                .ok()
                .flatten()
                .map(|result| result.map_err(|key| ImageCacheError::Asset(key.into())))
        })
    }

    fn request(
        &mut self,
        source: &ImageSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Result<Arc<RenderImage>, &'static str>> {
        let key = source.key();
        self.next += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.touched = self.next;
            return entry.result.clone();
        }
        // 64 thumbnails of at most 320 x 320 BGRA pixels; no encoded-byte cache.
        if self.entries.len() >= 64
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(id, _)| id.clone())
            && let Some(Ok(image)) = self
                .entries
                .remove(&oldest)
                .and_then(|mut entry| entry.result.take())
        {
            cx.drop_image(image, Some(window));
        }
        let generation = self.next;
        let cancel = CancellationToken::new();
        self.entries.insert(
            key.clone(),
            Entry {
                generation,
                touched: generation,
                cancel: cancel.clone(),
                result: None,
            },
        );
        let client = self.binding.client.clone();
        let slots = self.slots.clone();
        let task = self.binding.runtime.spawn(load::load(
            client,
            source.clone(),
            THUMBNAIL_SIDE,
            cancel,
            slots,
        ));
        cx.spawn_in(window, async move |images, cx| {
            let result = task.await.unwrap_or(Err("chat_image_unavailable"));
            let _ = images.update_in(cx, |images, window, _| {
                if let Some(entry) = images
                    .entries
                    .get_mut(&key)
                    .filter(|entry| entry.generation == generation)
                {
                    entry.result = Some(result);
                    window.refresh();
                }
            });
        })
        .detach();
        None
    }

    pub(crate) fn aspect_ratio(images: &Entity<Self>, source: &ImageSource, cx: &App) -> f32 {
        images
            .read(cx)
            .entries
            .get(&source.key())
            .and_then(|entry| entry.result.as_ref())
            .and_then(|result| result.as_ref().ok())
            .map(|image| {
                let size = image.size(0);
                size.width.0 as f32 / size.height.0.max(1) as f32
            })
            .unwrap_or(1.)
    }

    pub(crate) fn gallery(
        card: gpui_kit::component::attachment::Attachment,
        images: &Entity<Self>,
        sources: Vec<ImageSource>,
        index: usize,
    ) -> gpui_kit::component::attachment::Attachment {
        use gpui_kit::component::button::ButtonVariants as _;
        let id = sources[index].id();
        let keyboard_images = images.downgrade();
        let keyboard_sources = sources.clone();
        let images = images.downgrade();
        // The pinned Attachment click layer is pointer-only; Kit Button keeps keyboard access.
        card.id(format!("attachment-preview-{id}"))
            .accessibility_label(sources[index].name())
            .actions(
                AttachmentActions::new()
                    .top_auto()
                    .bottom_0()
                    .left_0()
                    .right_auto()
                    .child(
                        Button::new(format!("image-preview-{id}"))
                            .debug_selector(move || format!("image-preview-{id}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Eye)
                            .accessibility_label(tr("chat_image_preview"))
                            .tooltip(tr("chat_image_preview"))
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(images) = keyboard_images.upgrade() {
                                    preview::open_gallery(
                                        &images,
                                        keyboard_sources.clone(),
                                        index,
                                        window,
                                        cx,
                                    );
                                }
                            }),
                    ),
            )
            .on_click(move |_, window, cx| {
                if let Some(images) = images.upgrade() {
                    preview::open_gallery(&images, sources.clone(), index, window, cx);
                }
            })
    }
}

pub(crate) fn supports(path: &str) -> bool {
    source::file_format(path).is_some()
}

/// A package receives only the file source captured by its panel.
pub(crate) fn open_scoped(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    context: sailry_protocol::plugin::Context,
    path: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(worktree) = context.worktree else {
        return;
    };
    preview::open_sources(
        Binding { client, runtime },
        Arc::new(tokio::sync::Semaphore::new(2)),
        vec![ImageSource::File {
            worktree,
            path,
            context: Some(context),
        }],
        0,
        window,
        cx,
    );
}
