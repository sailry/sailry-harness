use super::*;
use gpui_kit::component::attachment::Attachment as Card;

pub(crate) fn card(
    source: &ImageSource,
    images: &Entity<Images>,
    gallery: Vec<ImageSource>,
    index: usize,
    cx: &App,
) -> Card {
    let single = gallery.len() == 1;
    let ratio = if single {
        Images::aspect_ratio(images, source, cx)
    } else {
        1.
    };
    let width = if single { 320. * ratio.min(1.) } else { 160. };
    let selector = format!("image-media-{}", source.id());
    let card = Card::new()
        .axis(Axis::Vertical)
        .xsmall()
        .w(px(width))
        .h(px(width / ratio))
        .max_w_full()
        .p_0()
        .gap_1()
        .border_0()
        .highlight(false)
        .bg(gpui_kit::transparent_black())
        .media(
            Images::media(images, source.clone(), cx)
                .w_full()
                .aspect_ratio(ratio)
                .overlay(div().size_full().debug_selector(move || selector.clone()))
                .rounded(cx.theme().radius),
        );
    Images::gallery(card, images, gallery, index)
}
