use super::{
    Package,
    decoration::asset,
    package::{Decoration, Fit},
};
use gpui_kit::{Size, component::*, *};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub fn preview(package: &Package, dark: bool, cx: &App) -> AnyElement {
    let mut theme = cx.theme().clone();
    theme.apply_config(if dark { &package.dark } else { &package.light });
    // Gallery and import cards share a one-pixel Kit border.
    let radius = (cx.theme().radius_lg - px(1.)).max(px(0.));
    div()
        .id(format!("theme-preview-{}-{dark}", package.id))
        .debug_selector({
            let id = package.id.clone();
            move || format!("theme-preview-surface-{id}-{dark}")
        })
        .relative()
        .w_full()
        .h_24()
        .rounded_t(radius)
        .overflow_hidden()
        .bg(theme.background)
        .children(
            asset(package, "background.window", dark)
                .or_else(|| asset(package, "background.content", dark))
                .map(|(decoration, image)| {
                    div()
                        .absolute()
                        .inset_0()
                        .child(wallpaper(decoration, image, radius))
                }),
        )
        .child(
            h_flex()
                .relative()
                .size_full()
                .child(
                    v_flex()
                        .relative()
                        .h_full()
                        .w_12()
                        .p_2()
                        .gap_2()
                        .overflow_hidden()
                        // Kit clamps each radius to half the painted rectangle.
                        // A wider inert fill retains the outer curve in this narrow column.
                        .child(
                            div()
                                .debug_selector({
                                    let id = package.id.clone();
                                    move || format!("theme-preview-sidebar-{id}-{dark}")
                                })
                                .absolute()
                                .left_0()
                                .top_0()
                                .h_full()
                                .w_12()
                                .min_w(radius * 2.)
                                .rounded_tl(radius)
                                .bg(theme.sidebar),
                        )
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
                                .max_w_full()
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

#[derive(Default)]
struct Wallpaper {
    key: Option<(ImageId, Size<DevicePixels>, f32)>,
    image: Option<Arc<RenderImage>>,
}

fn wallpaper(decoration: Decoration, source: Arc<RenderImage>, radius: Pixels) -> impl IntoElement {
    // Kit's rectangular content masks cannot round aligned or tiled child images.
    // Compose only this inert thumbnail's wallpaper, then let GPUI round one sprite.
    // Cache at the actual device size, retaining package fit, alignment and opacity.
    div().size_full().opacity(decoration.opacity).child(
        canvas(
            move |bounds, window, cx| {
                let scale = window.scale_factor();
                let dimensions = size(
                    DevicePixels((f32::from(bounds.size.width) * scale).ceil() as i32),
                    DevicePixels((f32::from(bounds.size.height) * scale).ceil() as i32),
                );
                let cached = window.use_keyed_state("wallpaper", cx, |_, _| Wallpaper::default());
                cached.update(cx, |cached, _| {
                    let key = (source.id, dimensions, scale);
                    if cached.key != Some(key) {
                        if let Some(previous) = cached.image.take()
                            && let Err(error) = window.drop_image(previous)
                        {
                            eprintln!("could not release theme thumbnail: {error}");
                        }
                        let pixels = compose(&decoration, &source, dimensions, scale);
                        cached.image =
                            Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])));
                        cached.key = Some(key);
                    }
                    cached.image.as_ref().unwrap().clone()
                })
            },
            move |bounds, image, window, _| {
                let corners = Corners {
                    top_left: radius,
                    top_right: radius,
                    ..Corners::default()
                };
                if let Err(error) = window.paint_image(bounds, bounds, corners, image, 0, false) {
                    eprintln!("could not paint theme thumbnail: {error}");
                }
            },
        )
        .size_full(),
    )
}

fn compose(
    decoration: &Decoration,
    source: &RenderImage,
    dimensions: Size<DevicePixels>,
    scale: f32,
) -> image::RgbaImage {
    let width = u32::from(dimensions.width).max(1);
    let height = u32::from(dimensions.height).max(1);
    let source_size = source.size(0);
    let source = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
        u32::from(source_size.width),
        u32::from(source_size.height),
        source.as_bytes(0).unwrap(),
    )
    .unwrap();
    let target = match decoration.fit {
        Fit::Tile => {
            let width = f32::from(decoration.tile_size) * scale;
            size(
                width,
                width * source.height() as f32 / source.width() as f32,
            )
        }
        fit => {
            let fit = match fit {
                Fit::Cover => ObjectFit::Cover,
                Fit::Contain => ObjectFit::Contain,
                Fit::Fill => ObjectFit::Fill,
                Fit::Tile => unreachable!(),
            };
            let bounds = Bounds::new(
                point(px(0.), px(0.)),
                size(px(width as f32), px(height as f32)),
            );
            fit.get_bounds(bounds, source_size).size.map(f32::from)
        }
    };
    let origin = point(
        (width as f32 - target.width) * decoration.alignment[0],
        (height as f32 - target.height) * decoration.alignment[1],
    );
    // Sample only the thumbnail's visible pixels, even for extreme source aspect ratios.
    image::RgbaImage::from_fn(width, height, |x, y| {
        let (u, v) = if matches!(decoration.fit, Fit::Tile) {
            (
                ((x as f32 + 0.5) / target.width).fract(),
                ((y as f32 + 0.5) / target.height).fract(),
            )
        } else {
            (
                (x as f32 + 0.5 - origin.x) / target.width,
                (y as f32 + 0.5 - origin.y) / target.height,
            )
        };
        image::imageops::sample_bilinear(&source, u, v).unwrap_or(image::Rgba([0; 4]))
    })
}
