//! Native flows of an HTML block. Their text maps to nothing in the block's
//! source, so it holds no caret; a click on the block reveals that source.
use super::{Overlay, Part, Stated, caret_target, text_element};
use crate::markdown::html::layout::{Align, Flow, Item, Layout, Picture};
use crate::markdown::{style::Theme, typography::Typography};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, div, img, px,
};

const FLOW_GAP: f32 = 8.;
const ITEM_GAP: f32 = 4.;

pub(super) fn flows(
    layout: &Layout,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let inner = Overlay {
        selection: None,
        layouts: None,
        placeholder: None,
        hovered_link: None,
        on_link_hover: None,
        annotations: &[],
        ..overlay.at(Part::Code)
    };
    let column = div()
        .flex()
        .flex_col()
        .gap(px(FLOW_GAP))
        .w_full()
        .min_w_0()
        .children(
            layout
                .flows
                .iter()
                .enumerate()
                .map(|(index, flow)| line(index, flow, inner, typography, theme, cx)),
        );
    caret_target(column.into_any_element(), overlay, theme)
}

fn line(
    index: usize,
    flow: &Flow,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let metrics = flow
        .heading
        .map_or(typography.body, |level| typography.heading(level));
    let single = flow.items.len() == 1;
    let row = div()
        .id(("html-flow", index))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(ITEM_GAP))
        .w_full()
        .min_w_0();
    let row = match flow.align {
        Align::Start => row,
        Align::Center => row.justify_center().text_center(),
        Align::End => row.justify_end().text_right(),
    };
    row.children(flow.items.iter().enumerate().map(|(at, item)| {
        let cell = div().id(("html-item", at)).min_w_0().max_w_full();
        match item {
            Item::Text(text) => cell
                .when(single, |cell| cell.w_full())
                .child(text_element(
                    text,
                    metrics.size(),
                    metrics.line_height(),
                    metrics.weight,
                    overlay,
                    theme,
                    cx,
                ))
                .into_any_element(),
            Item::Picture(picture) => {
                let content = image(picture, metrics.size(), overlay, theme, cx);
                match &picture.link {
                    Some(url) => {
                        let url = SharedString::from(url.clone());
                        let on_link = overlay.on_link.cloned();
                        cell.cursor_pointer()
                            .on_click(move |_, window, cx| match &on_link {
                                Some(on_link) => on_link(&url, window, cx),
                                None => cx.open_url(&url),
                            })
                            .child(content)
                            .into_any_element()
                    }
                    None => cell.child(content).into_any_element(),
                }
            }
        }
    }))
    .into_any_element()
}

/// Local pictures use the surface's confined Markdown image path; remote ones
/// use the same element as Markdown images and show their name if it fails.
fn image(picture: &Picture, size: f32, overlay: Overlay, theme: &Theme, cx: &App) -> AnyElement {
    let name = SharedString::from(match (&picture.alt, &picture.link) {
        (alt, _) if !alt.is_empty() => alt.clone(),
        (_, Some(link)) => link.clone(),
        _ => picture.url.clone(),
    });
    let color = match picture.link {
        Some(_) => theme.link,
        None => theme.text_muted,
    };
    let label = move || {
        div()
            .text_size(px(size))
            .text_color(color)
            .child(name.clone())
            .into_any_element()
    };
    if picture.url.starts_with("https://") || picture.url.starts_with("http://") {
        let mut element = img(SharedString::from(picture.url.clone()))
            .max_w_full()
            .with_fallback(label);
        if let Some(width) = picture.width {
            element = element.w(px(width));
        }
        if let Some(height) = picture.height {
            element = element.h(px(height));
        }
        return element.into_any_element();
    }
    // Without a surface there is no document directory to confine a path to.
    let Some(render) = overlay.on_image else {
        return label();
    };
    let stated = Stated {
        width: picture.width,
        height: picture.height,
    };
    render(&picture.url, &[picture.url.as_str()], 0, stated, cx)
}
