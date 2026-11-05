use super::*;
use gpui_kit::component::attachment::Attachment as Card;
use sailry_content::markdown::render::Stated;

/// How a card sits among its siblings and answers a click.
#[derive(Clone, Copy, Default)]
pub(crate) struct Presentation {
    /// One of several pictures, shown at the shared group size.
    pub compact: bool,
    /// A click opens the gallery instead of reaching the surrounding editor.
    pub preview: bool,
    pub stated: Stated,
}

pub(crate) fn card(
    source: &ImageSource,
    images: &Entity<Images>,
    gallery: Vec<ImageSource>,
    index: usize,
    presentation: Presentation,
    cx: &App,
) -> AnyElement {
    let Presentation {
        compact,
        preview,
        stated,
    } = presentation;
    let single = !compact;
    let ratio = if single {
        Images::aspect_ratio(images, source, cx)
    } else {
        1.
    };
    let width = if single {
        320. * ratio.min(1.)
    } else {
        GROUP_WIDTH
    };
    let (width, height) = sized(stated, width, ratio);
    let ratio = width / height;
    let selector = format!("image-media-{}", source.id());
    let card = Card::new()
        .rounded(cx.theme().radius_2xl())
        .axis(Axis::Vertical)
        .xsmall()
        .w(px(width))
        .h(px(height))
        .max_w_full()
        .p_0()
        .gap_1()
        .border_0()
        .bg(gpui_kit::transparent_black())
        .media(
            Images::media(images, source.clone(), cx)
                .w_full()
                .aspect_ratio(ratio)
                .overlay(div().size_full().debug_selector(move || selector.clone()))
                .rounded(cx.theme().radius_2xl()),
        );
    // Without preview the click belongs to the surrounding editor.
    match preview {
        true => Images::gallery(card, images, gallery, index, cx).into_any_element(),
        false => card.into_any_element(),
    }
}

/// A document's stated sides win; an unset one follows the picture's proportions.
pub(super) fn sized(stated: Stated, natural: f32, ratio: f32) -> (f32, f32) {
    match (stated.width, stated.height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, width / ratio),
        (None, Some(height)) => (height * ratio, height),
        (None, None) => (natural, natural / ratio),
    }
}
