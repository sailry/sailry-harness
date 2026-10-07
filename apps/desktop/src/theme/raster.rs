use super::package::{Decoration, Fit};
use gpui_kit::{Size, component::*, *};
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    image: ImageId,
    dimensions: Size<DevicePixels>,
    scale: f32,
    fit: std::mem::Discriminant<Fit>,
    alignment: [f32; 2],
    tile_size: u16,
    strip_height: Option<u16>,
}

#[derive(Default)]
struct Cached {
    key: Option<Key>,
    image: Option<Arc<RenderImage>>,
}

pub(super) fn rounded(
    decoration: Decoration,
    source: Arc<RenderImage>,
    corners: Corners<Pixels>,
    strip_height: Option<u16>,
) -> impl IntoElement {
    // Kit's rectangular content mask does not round child images. Compose an
    // inert image at the full control bounds so GPUI rounds the outer sprite,
    // including aligned artwork and strips shorter than the control itself.
    div().size_full().opacity(decoration.opacity).child(
        canvas(
            move |bounds, window, cx| {
                let scale = window.scale_factor();
                let dimensions = size(
                    DevicePixels((f32::from(bounds.size.width) * scale).ceil() as i32),
                    DevicePixels((f32::from(bounds.size.height) * scale).ceil() as i32),
                );
                let cached = window.use_keyed_state("theme-raster", cx, |_, _| Cached::default());
                cached.update(cx, |cached, _| {
                    let key = Key {
                        image: source.id,
                        dimensions,
                        scale,
                        fit: std::mem::discriminant(&decoration.fit),
                        alignment: decoration.alignment,
                        tile_size: decoration.tile_size,
                        strip_height,
                    };
                    if cached.key != Some(key) {
                        if let Some(previous) = cached.image.take()
                            && let Err(error) = window.drop_image(previous)
                        {
                            eprintln!("could not release rounded theme image: {error}");
                        }
                        let pixels = compose(&decoration, &source, dimensions, scale, strip_height);
                        cached.image =
                            Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])));
                        cached.key = Some(key);
                    }
                    cached.image.as_ref().unwrap().clone()
                })
            },
            move |bounds, image, window, _| {
                if let Err(error) = window.paint_image(bounds, bounds, corners, image, 0, false) {
                    eprintln!("could not paint rounded theme image: {error}");
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
    strip_height: Option<u16>,
) -> image::RgbaImage {
    let width = u32::from(dimensions.width).max(1);
    let height = u32::from(dimensions.height).max(1);
    let viewport_height = strip_height.map_or(height as f32, |strip| {
        (f32::from(strip) * scale).min(height as f32)
    });
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
                size(px(width as f32), px(viewport_height)),
            );
            fit.get_bounds(bounds, source_size).size.map(f32::from)
        }
    };
    let origin = point(
        (width as f32 - target.width) * decoration.alignment[0],
        (viewport_height - target.height) * decoration.alignment[1],
    );
    // Sample only the surface's visible pixels, even for extreme source aspect ratios.
    image::RgbaImage::from_fn(width, height, |x, y| {
        if y as f32 + 0.5 >= viewport_height {
            return image::Rgba([0; 4]);
        }
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
