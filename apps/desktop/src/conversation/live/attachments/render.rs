use super::images::{ImageSource, Images};
use super::*;
use gpui_kit::component::{
    attachment::{
        Attachment as Card, AttachmentActions, AttachmentContent, AttachmentDescription,
        AttachmentGroup, AttachmentMedia, AttachmentStatus, AttachmentTitle,
    },
    button::{Button, ButtonVariants},
    message::MessageAlignment,
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(in crate::conversation::live) fn attachment_drafts(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.attachments.items.is_empty() {
            return None;
        }
        let blocked = self.attachments_blocked();
        let sources: Vec<_> = self.attachments.items.iter().map(draft_source).collect();
        let gallery: Vec<_> = sources
            .iter()
            .filter(|source| source.kind().is_some())
            .cloned()
            .collect();
        Some(
            AttachmentGroup::new("composer-attachments")
                .with_edge_fade(cx.theme().muted)
                .children(
                    self.attachments
                        .items
                        .iter()
                        .zip(sources)
                        .map(|(item, source)| {
                            let key = item.key;
                            let (status, detail, progress) =
                                draft_status(&item.status, item.removing);
                            let removable = !item.removing
                                && !matches!(
                                    item.status,
                                    Status::Publishing(_)
                                        | Status::Removing
                                        | Status::Uncertain(_)
                                        | Status::Rejected(_)
                                );
                            let retry = status.is_failed() && !item.pending && !blocked;
                            let card = if source.kind().is_some() {
                                let index = gallery
                                    .iter()
                                    .position(|value| value.id() == source.id())
                                    .unwrap();
                                Images::gallery(
                                    Card::new().axis(Axis::Vertical).media(
                                        Images::media(&self.images, source.clone(), cx)
                                            .rounded(cx.theme().radius_2xl())
                                            .overlay(media_bounds(key)),
                                    ),
                                    &self.images,
                                    gallery.clone(),
                                    index,
                                )
                            } else {
                                Card::new()
                                    .media(
                                        AttachmentMedia::new()
                                            .child(crate::ui::file_icon::render(
                                                &item.name,
                                                &source.mime(),
                                                rems(1.25),
                                            ))
                                            .overlay(media_bounds(key)),
                                    )
                                    .content(
                                        AttachmentContent::new()
                                            .title(AttachmentTitle::new(item.name.clone()))
                                            .description(AttachmentDescription::new(
                                                source.description(),
                                            )),
                                    )
                            };
                            let card = card
                                .rounded(cx.theme().radius_2xl())
                                .id(format!("composer-attachment-{key}"))
                                .status(status)
                                .tooltip(detail.map(tr).unwrap_or_else(|| item.name.clone().into()))
                                .when_some(progress, Card::progress)
                                .when(retry, |card| {
                                    card.on_retry(cx.listener(move |view, _, window, cx| {
                                        cx.stop_propagation();
                                        view.upload_attachment(key, window, cx)
                                    }))
                                })
                                .when(removable && !blocked, |card| {
                                    card.on_remove(cx.listener(move |view, _, window, cx| {
                                        cx.stop_propagation();
                                        view.remove_attachment(key, window, cx)
                                    }))
                                });
                            div()
                                .id(("attachment-card", key))
                                .flex_shrink_0()
                                .debug_selector(move || format!("attachment-card-{key}"))
                                .child(card)
                                .into_any_element()
                        }),
                )
                .into_any_element(),
        )
    }
}

fn media_bounds(key: usize) -> Div {
    div()
        .absolute()
        .inset_0()
        .size_full()
        .debug_selector(move || format!("attachment-media-{key}"))
}

fn draft_source(item: &Item) -> ImageSource {
    match &item.status {
        Status::Ready(attachment) => ImageSource::from(attachment),
        _ => ImageSource::Local {
            key: item.key,
            source: item
                .source
                .clone()
                .expect("unpublished attachment has local bytes"),
        },
    }
}

fn draft_status(
    status: &Status,
    removing: bool,
) -> (AttachmentStatus, Option<&'static str>, Option<f32>) {
    match status {
        Status::Preparing => (
            AttachmentStatus::Uploading,
            Some("files_upload_preparing"),
            None,
        ),
        Status::Sending { copied, size } => (
            AttachmentStatus::Uploading,
            Some("files_upload_sending"),
            Some(if *size == 0 {
                100.
            } else {
                *copied as f32 / *size as f32 * 100.
            }),
        ),
        Status::Publishing(_) => (
            AttachmentStatus::Processing,
            Some(if removing {
                "chat_attachment_removing"
            } else {
                "files_upload_publishing"
            }),
            None,
        ),
        Status::Staged => (AttachmentStatus::Pending, None, None),
        Status::Ready(_) | Status::Removed => (AttachmentStatus::Complete, None, None),
        Status::Failed(key) => (AttachmentStatus::Failed, Some(*key), None),
        Status::Uncertain(_) => (
            AttachmentStatus::Failed,
            Some("chat_attachment_unknown"),
            None,
        ),
        Status::Rejected(_) => (
            AttachmentStatus::Failed,
            Some("chat_attachment_remove_failed"),
            None,
        ),
        Status::Removing => (
            AttachmentStatus::Processing,
            Some("chat_attachment_removing"),
            None,
        ),
    }
}

pub(in crate::conversation::live) fn links(
    attachments: &[Attachment],
    binding: &Binding,
    images: &Entity<Images>,
    cx: &App,
) -> AnyElement {
    source_links(
        attachments.iter().map(ImageSource::from).collect(),
        binding,
        images,
        MessageAlignment::Start,
        cx,
    )
}

pub(in crate::conversation::live) fn image_links(
    attachments: &[sailry_protocol::conversation::Image],
    session: sailry_protocol::SessionId,
    binding: &Binding,
    images: &Entity<Images>,
    cx: &App,
) -> AnyElement {
    source_links(
        attachments
            .iter()
            .map(|image| ImageSource::Image {
                session,
                image: image.clone(),
            })
            .collect(),
        binding,
        images,
        MessageAlignment::Start,
        cx,
    )
}

pub(in crate::conversation::live) fn source_links(
    sources: Vec<ImageSource>,
    binding: &Binding,
    images: &Entity<Images>,
    alignment: MessageAlignment,
    cx: &App,
) -> AnyElement {
    let (gallery, files): (Vec<_>, Vec<_>) = sources
        .into_iter()
        .partition(|source| source.kind().is_some());
    v_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .when(!files.is_empty(), |content| {
            let id = files[0].id();
            content.child(
                AttachmentGroup::new(format!("message-files-{id}"))
                    .flex_wrap()
                    .when(alignment == MessageAlignment::End, |group| {
                        group.justify_end()
                    })
                    .children(
                        files
                            .into_iter()
                            .map(|source| file_card(source, binding, images, cx)),
                    ),
            )
        })
        .when(!gallery.is_empty(), |content| {
            let id = gallery[0].id();
            content.child(
                AttachmentGroup::new(format!("message-images-{id}"))
                    .flex_wrap()
                    .when(alignment == MessageAlignment::End, |group| {
                        group.justify_end()
                    })
                    .children(gallery.iter().enumerate().map(|(index, source)| {
                        let selector = format!("image-card-{}", source.id());
                        div()
                            .debug_selector(move || selector.clone())
                            .flex_shrink_0()
                            .max_w_full()
                            .child(images::card(
                                source,
                                images,
                                gallery.clone(),
                                index,
                                gallery.len() > 1,
                                cx,
                            ))
                            .into_any_element()
                    })),
            )
        })
        .into_any_element()
}

fn file_card(
    source: ImageSource,
    binding: &Binding,
    images: &Entity<Images>,
    cx: &App,
) -> AnyElement {
    let binding = binding.clone();
    let id = source.id();
    let selector = id.clone();
    let card = Card::new()
        .rounded(cx.theme().radius_2xl())
        .media(Images::media(images, source.clone(), cx))
        .content(
            AttachmentContent::new()
                .title(AttachmentTitle::new(source.name()))
                .description(AttachmentDescription::new(source.description())),
        )
        .actions(
            AttachmentActions::new().child(
                Button::new(format!("attachment-download-{id}"))
                    .debug_selector(move || format!("attachment-download-{selector}"))
                    .ghost()
                    .xsmall()
                    .icon(IconName::ArrowDown)
                    .tooltip(tr("files_download"))
                    .accessibility_label(format!("{}: {}", tr("files_download"), source.name()))
                    .on_click({
                        let source = source.clone();
                        move |_, window, cx| {
                            source.download(
                                binding.client.clone(),
                                binding.runtime.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            ),
        );
    div()
        .debug_selector(move || format!("file-card-{id}"))
        .child(card)
        .into_any_element()
}
