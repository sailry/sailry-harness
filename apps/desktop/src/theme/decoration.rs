use super::{
    Catalog, Package,
    package::{Decoration, Fit},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{component::*, *};
use std::sync::Arc;

pub(super) fn asset(
    package: &Package,
    slot: &str,
    dark: bool,
) -> Option<(Decoration, Arc<RenderImage>)> {
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

pub(super) fn selected(slot: &str, cx: &App) -> Option<(Decoration, Arc<RenderImage>)> {
    let catalog = cx.try_global::<Catalog>()?;
    asset(
        &catalog.packages[catalog.selected],
        slot,
        cx.theme().is_dark(),
    )
}

// Kit 0c830f4's Img supports fit but has no alignment or repeat API.
// This decorative canvas supplies alignment and tiling to GPUI's image painter. It
// registers no hitbox or actions; framework controls retain their behavior.
fn picture(decoration: Decoration, image: Arc<RenderImage>) -> impl IntoElement {
    div()
        .size_full()
        .overflow_hidden()
        .opacity(decoration.opacity)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if matches!(decoration.fit, Fit::Tile) {
                        let image_size = image.size(0);
                        let width = f32::from(decoration.tile_size);
                        let height = width * u32::from(image_size.height) as f32
                            / u32::from(image_size.width) as f32;
                        for target in tiles(bounds, size(px(width), px(height))) {
                            if let Err(error) = window.paint_image(
                                bounds,
                                target,
                                Corners::default(),
                                image.clone(),
                                0,
                                false,
                            ) {
                                eprintln!("could not paint theme tile: {error}");
                                break;
                            }
                        }
                        return;
                    }
                    let fit = match decoration.fit {
                        Fit::Cover => ObjectFit::Cover,
                        Fit::Contain => ObjectFit::Contain,
                        Fit::Fill => ObjectFit::Fill,
                        Fit::Tile => unreachable!(),
                    };
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

fn tiles(
    bounds: Bounds<Pixels>,
    tile: gpui_kit::Size<Pixels>,
) -> impl Iterator<Item = Bounds<Pixels>> {
    let columns = (bounds.size.width / tile.width).ceil() as usize;
    let rows = (bounds.size.height / tile.height).ceil() as usize;
    (0..rows).flat_map(move |row| {
        (0..columns).map(move |column| {
            Bounds::new(
                bounds.origin + point(tile.width * column as f32, tile.height * row as f32),
                tile,
            )
        })
    })
}

#[cfg(test)]
mod tiling {
    use super::tiles;
    use gpui_kit::{Bounds, point, px, size};

    #[test]
    fn covers_resized_bounds_without_stretching() {
        let origin = point(px(11.), px(17.));
        let tile = size(px(64.), px(32.));
        for dimensions in [size(px(130.), px(65.)), size(px(193.), px(99.))] {
            let bounds = Bounds::new(origin, dimensions);
            let targets = tiles(bounds, tile).collect::<Vec<_>>();
            let columns = (dimensions.width / tile.width).ceil() as usize;
            let rows = (dimensions.height / tile.height).ceil() as usize;
            assert_eq!(targets.len(), columns * rows);
            assert_eq!(targets[0].origin, origin);
            for (index, target) in targets.iter().enumerate() {
                assert_eq!(target.size, tile);
                assert_eq!(
                    target.left(),
                    origin.x + tile.width * (index % columns) as f32
                );
                assert_eq!(
                    target.top(),
                    origin.y + tile.height * (index / columns) as f32
                );
            }
            let last = targets.last().unwrap();
            assert!(last.right() >= bounds.right() && last.bottom() >= bounds.bottom());
            assert!(last.left() < bounds.right() && last.top() < bounds.bottom());
        }
        assert_eq!(
            tiles(Bounds::new(origin, size(px(0.), px(0.))), tile).count(),
            0
        );
    }
}

pub(super) fn corner(slot: &'static str, cx: &App) -> Option<AnyElement> {
    let (decoration, image) = selected(slot, cx)?;
    Some(
        div()
            .debug_selector(move || format!("theme-{slot}"))
            .absolute()
            .size(px(f32::from(decoration.height)))
            .when(slot.ends_with(".top_left"), |corner| {
                corner.top_0().left_0()
            })
            .when(slot.ends_with(".bottom_right"), |corner| {
                corner.bottom_0().right_0()
            })
            .child(picture(decoration, image))
            .into_any_element(),
    )
}

pub fn new_session_button(button: button::Button, cx: &App) -> AnyElement {
    let Some((decoration, image)) = selected("button.new_session", cx) else {
        return button.into_any_element();
    };
    // Keep the complete Kit button underneath an eventless top-edge ornament.
    div()
        .relative()
        .w_full()
        .child(button)
        .child(
            div()
                .debug_selector(|| "theme-button.new_session".into())
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(f32::from(decoration.height)))
                .rounded(cx.theme().radius)
                .overflow_hidden()
                .child(picture(decoration, image)),
        )
        .into_any_element()
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
    if selected(&format!("icon.{key}"), cx).is_some() {
        sized_icon(key, rems(1.), fallback, cx)
    } else {
        Icon::new(fallback).into_any_element()
    }
}

pub(super) fn sized_icon(key: &str, size: Rems, fallback: impl Into<Icon>, cx: &App) -> AnyElement {
    match selected(&format!("icon.{key}"), cx) {
        Some((decoration, image)) => div()
            .debug_selector({
                let key = key.to_owned();
                move || format!("theme-icon.{key}")
            })
            .size(size)
            .flex_shrink_0()
            .child(picture(decoration, image))
            .into_any_element(),
        None => Icon::new(fallback).size(size).into_any_element(),
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
