use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::ImageDecoder;
use std::{collections::BTreeMap, io::Cursor, sync::Arc};

#[derive(Default)]
pub(super) struct Cache {
    images: BTreeMap<u32, (protocol::Image, Arc<RenderImage>)>,
}

pub(super) struct Placement {
    pub z: i32,
    bounds: Bounds<Pixels>,
    image_bounds: Bounds<Pixels>,
    image: Arc<RenderImage>,
}

impl Cache {
    pub fn clear(&mut self, window: &mut Window) {
        for (_, image) in std::mem::take(&mut self.images).into_values() {
            let _ = window.drop_image(image);
        }
    }
    #[cfg(test)]
    pub fn image(&self, id: u32) -> Option<Arc<RenderImage>> {
        self.images.get(&id).map(|(_, image)| image.clone())
    }

    pub fn prepare(
        &mut self,
        screen: &protocol::Screen,
        metrics: viewport::Metrics,
        offset: Pixels,
        window: &mut Window,
    ) -> Vec<Placement> {
        self.images.retain(|id, (source, image)| {
            let retain = screen
                .graphics
                .images
                .iter()
                .any(|candidate| candidate.id == *id && candidate == source);
            if !retain {
                let _ = window.drop_image(image.clone());
            }
            retain
        });
        for image in &screen.graphics.images {
            if self
                .images
                .get(&image.id)
                .is_some_and(|(source, _)| source == image)
            {
                continue;
            }
            if let Some(rendered) = decode(image) {
                self.images.insert(image.id, (image.clone(), rendered));
            }
        }
        screen
            .graphics
            .placements
            .iter()
            .filter_map(|placement| {
                let (_, image) = self.images.get(&placement.image)?;
                let [x, y, width, height] = placement.source;
                if width == 0 || height == 0 {
                    return None;
                }
                let scale = size(
                    metrics.cell.width / placement.cell[0].max(1) as f32,
                    metrics.cell.height / placement.cell[1].max(1) as f32,
                );
                let origin = point(
                    metrics.bounds.left()
                        + metrics.cell.width * placement.column as f32
                        + scale.width * placement.offset[0] as f32,
                    metrics.bounds.top()
                        + offset
                        + metrics.cell.height
                            * (screen.scrollback.len() as i64 + i64::from(placement.row)) as f32
                        + scale.height * placement.offset[1] as f32,
                );
                let size = size(
                    scale.width * placement.size[0] as f32,
                    scale.height * placement.size[1] as f32,
                );
                let [column, row, columns, rows] = placement.tile.unwrap_or([0, 0, 1, 1]);
                let source_scale = size.width * columns as f32 / width as f32;
                let source_scale_y = size.height * rows as f32 / height as f32;
                Some(Placement {
                    z: placement.z,
                    bounds: Bounds::new(origin, size),
                    image_bounds: Bounds::new(
                        point(
                            origin.x - size.width * column as f32 - source_scale * x as f32,
                            origin.y - size.height * row as f32 - source_scale_y * y as f32,
                        ),
                        gpui_kit::size(
                            source_scale * image.size(0).width.0 as f32,
                            source_scale_y * image.size(0).height.0 as f32,
                        ),
                    ),
                    image: image.clone(),
                })
            })
            .collect()
    }
}

fn decode(source: &protocol::Image) -> Option<Arc<RenderImage>> {
    let bytes = STANDARD.decode(source.png.as_ref()).ok()?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(protocol::MAX_GRAPHICS_BYTES as u64);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    if decoder.dimensions() != (source.width, source.height) {
        return None;
    }
    let mut pixels = image::DynamicImage::from_decoder(decoder)
        .ok()?
        .into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

pub(super) fn paint(placements: &[Placement], layer: i8, window: &mut Window) {
    for placement in placements {
        let target = if placement.z < i32::MIN / 2 {
            -1
        } else if placement.z < 0 {
            0
        } else {
            1
        };
        if target != layer {
            continue;
        }
        if let Err(error) = window.paint_image(
            placement.bounds,
            placement.image_bounds,
            Corners::default(),
            placement.image.clone(),
            0,
            false,
        ) {
            eprintln!("could not paint terminal image: {error}");
        }
    }
}
