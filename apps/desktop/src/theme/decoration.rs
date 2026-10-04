use super::{
    Catalog, Package,
    package::{Decoration, Fit},
};
use gpui_kit::{component::*, *};
use std::sync::Arc;

fn asset(package: &Package, slot: &str, dark: bool) -> Option<(Decoration, Arc<RenderImage>)> {
    let manifest = package.manifest.as_ref()?;
    let variant = if dark {
        &manifest.dark
    } else {
        &manifest.light
    };
    let decoration = variant.assets.get(slot)?;
    Some((
        decoration.clone(),
        package.images.get(&decoration.path)?.clone(),
    ))
}

fn selected(slot: &str, cx: &App) -> Option<(Decoration, Arc<RenderImage>)> {
    let catalog = cx.try_global::<Catalog>()?;
    asset(
        &catalog.packages[catalog.selected],
        slot,
        cx.theme().is_dark(),
    )
}

// Kit b79f4ce / GPUI 0.3.4 Img supports fit but has no image-alignment API.
// This decorative canvas only supplies alignment to GPUI's image painter. It
// registers no hitbox or actions; framework controls retain their behavior.
fn picture(decoration: Decoration, image: Arc<RenderImage>) -> impl IntoElement {
    let fit = match decoration.fit {
        Fit::Cover => ObjectFit::Cover,
        Fit::Contain => ObjectFit::Contain,
        Fit::Fill => ObjectFit::Fill,
    };
    div()
        .size_full()
        .overflow_hidden()
        .opacity(decoration.opacity)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let mut target = fit.get_bounds(bounds, image.size(0));
                    target.origin = bounds.origin
                        + point(
                            (bounds.size.width - target.size.width) * decoration.alignment[0],
                            (bounds.size.height - target.size.height) * decoration.alignment[1],
                        );
                    if let Err(error) = window.paint_image(
                        bounds,
                        target,
                        Corners::default(),
                        image.clone(),
                        0,
                        false,
                    ) {
                        eprintln!("could not paint theme image: {error}");
                    }
                },
            )
            .size_full(),
        )
}

pub fn background(slot: &'static str, cx: &App) -> Option<AnyElement> {
    let (decoration, image) = selected(slot, cx)?;
    Some(
        div()
            .debug_selector(move || format!("theme-{slot}"))
            .absolute()
            .inset_0()
            .child(picture(decoration, image))
            .into_any_element(),
    )
}

pub fn banner(slot: &'static str, cx: &App) -> Option<AnyElement> {
    let (decoration, image) = selected(slot, cx)?;
    Some(
        div()
            .debug_selector(move || format!("theme-{slot}"))
            .w_full()
            .h(px(f32::from(decoration.height)))
            .flex_shrink_0()
            .child(picture(decoration, image))
            .into_any_element(),
    )
}

pub fn icon(key: &str, fallback: impl Into<Icon>, cx: &App) -> AnyElement {
    match selected(&format!("icon.{key}"), cx) {
        Some((decoration, image)) => div()
            .size_4()
            .flex_shrink_0()
            .child(picture(decoration, image))
            .into_any_element(),
        None => Icon::new(fallback).into_any_element(),
    }
}

pub fn brand(size: Pixels, cx: &App) -> Option<AnyElement> {
    let (decoration, image) = selected("brand", cx)?;
    Some(
        div()
            .size(size)
            .flex_shrink_0()
            .child(picture(decoration, image))
            .into_any_element(),
    )
}

pub fn preview(package: &Package, dark: bool, cx: &App) -> AnyElement {
    let mut theme = cx.theme().clone();
    theme.apply_config(if dark { &package.dark } else { &package.light });
    div()
        .relative()
        .w_full()
        .h_24()
        .overflow_hidden()
        .bg(theme.background)
        .children(
            asset(package, "background.content", dark).map(|(decoration, image)| {
                div().absolute().inset_0().child(picture(decoration, image))
            }),
        )
        .child(
            h_flex()
                .relative()
                .size_full()
                .child(
                    v_flex()
                        .h_full()
                        .w_12()
                        .p_2()
                        .gap_2()
                        .bg(theme.sidebar)
                        .child(div().size_3().rounded_full().bg(theme.primary))
                        .children(
                            (0..3).map(|_| div().h_1().w_full().bg(theme.foreground.opacity(0.12))),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .p_3()
                        .gap_2()
                        .child(
                            div()
                                .h_2()
                                .w_16()
                                .rounded_full()
                                .bg(theme.foreground.opacity(0.3)),
                        )
                        .child(div().h_1().w_full().bg(theme.foreground.opacity(0.1)))
                        .child(div().flex_1())
                        .child(
                            h_flex()
                                .h_6()
                                .p_1()
                                .rounded(theme.radius)
                                .bg(theme.group_box)
                                .child(div().flex_1())
                                .child(div().size_4().rounded_full().bg(theme.primary)),
                        ),
                ),
        )
        .into_any_element()
}
