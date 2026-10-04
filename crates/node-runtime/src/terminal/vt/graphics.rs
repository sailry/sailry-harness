use super::*;
mod unicode;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageDecoder, ImageEncoder};
use libghostty_vt::{
    alloc::{Allocator, Bytes},
    kitty::graphics as kitty,
};
use sailry_protocol::terminal::{Graphics, Image, MAX_GRAPHICS_BYTES, Placement};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor as Reader,
};

pub(super) fn configure(terminal: &mut Ghostty<'static, 'static>) -> Result<(), Fault> {
    check(
        kitty::set_png_decoder(Some(Box::new(Png))),
        "configure Ghostty PNG decoder",
    )?;
    check(
        terminal.set_kitty_image_storage_limit((MAX_GRAPHICS_BYTES * 3 / 4) as u64),
        "enable Ghostty graphics",
    )?;
    Ok(())
}

// The pinned wrapper's bundled decoder does not size its output buffer. Decode
// through the workspace image library with the same terminal resource limit.
struct Png;
impl kitty::DecodePng for Png {
    fn decode_png<'alloc>(
        &mut self,
        alloc: &'alloc Allocator<'_>,
        data: &[u8],
    ) -> Option<kitty::DecodedImage<'alloc>> {
        let mut reader =
            image::ImageReader::with_format(Reader::new(data), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_GRAPHICS_BYTES as u64);
        reader.limits(limits);
        let decoder = reader.into_decoder().ok()?;
        let (width, height) = decoder.dimensions();
        if u64::from(width) * u64::from(height) * 4 > MAX_GRAPHICS_BYTES as u64 {
            return None;
        }
        let pixels = image::DynamicImage::from_decoder(decoder)
            .ok()?
            .into_rgba8();
        let mut data = Bytes::new_with_alloc(alloc, pixels.len()).ok()?;
        data.copy_from_slice(&pixels);
        Some(kitty::DecodedImage {
            width,
            height,
            data,
        })
    }
}

#[derive(Default)]
pub(super) struct Cache {
    images: BTreeMap<u32, Image>,
    screen: Option<ActiveScreen>,
}

impl Cache {
    pub fn project(
        &mut self,
        terminal: &Ghostty<'_, '_>,
        viewport: &Viewport,
    ) -> Result<Graphics, Fault> {
        let screen = check(terminal.active_screen(), "read Ghostty image screen")?;
        if self.screen != Some(screen) {
            self.images.clear();
            self.screen = Some(screen);
        }
        let storage = check(terminal.kitty_graphics(), "read Ghostty graphics")?;
        let mut iterator = check(
            kitty::PlacementIterator::new(),
            "create Ghostty placements iterator",
        )?;
        let mut placements = check(iterator.update(&storage), "read Ghostty placements")?;
        let history = history_rows(terminal)? as i64;
        let mut output = Graphics::default();
        let mut virtuals = Vec::new();
        let mut retained = BTreeSet::new();
        while let Some(placement) = placements.next() {
            let id = check(placement.image_id(), "read Ghostty image ID")?;
            let Some(image) = storage.image(id) else {
                continue;
            };
            let generation = check(image.generation(), "read Ghostty image generation")?;
            if self
                .images
                .get(&id)
                .is_none_or(|cached| cached.generation != generation)
            {
                let width = check(image.width(), "read Ghostty image width")?;
                let height = check(image.height(), "read Ghostty image height")?;
                let format = check(image.format(), "read Ghostty image format")?;
                let color = match format {
                    kitty::ImageFormat::Rgb => image::ExtendedColorType::Rgb8,
                    kitty::ImageFormat::Rgba => image::ExtendedColorType::Rgba8,
                    _ => return Err(fail("unsupported decoded Ghostty image format".into())),
                };
                let mut png = Vec::new();
                image::codecs::png::PngEncoder::new(&mut png)
                    .write_image(
                        check(image.data(), "read Ghostty image pixels")?,
                        width,
                        height,
                        color,
                    )
                    .map_err(|error| fail(format!("encode terminal image: {error}")))?;
                self.images.insert(
                    id,
                    Image {
                        id,
                        generation,
                        width,
                        height,
                        png: STANDARD.encode(png).into(),
                    },
                );
            }
            if check(placement.is_virtual(), "read Ghostty placement type")? {
                let grid = check(
                    placement.grid_size(&image, terminal),
                    "read virtual placement size",
                )?;
                let source = check(placement.source_rect(&image), "read virtual placement crop")?;
                virtuals.push(unicode::Virtual {
                    image: id,
                    id: check(placement.placement_id(), "read virtual placement ID")?,
                    grid: [grid.cols, grid.rows],
                    source: [source.x, source.y, source.width, source.height],
                    z: check(placement.z(), "read virtual placement layer")?,
                });
                continue;
            }
            let rectangle = check(
                placement.rect(&image, terminal),
                "read Ghostty image anchor",
            )?;
            let Some(point) = check(
                terminal.point_from_grid_ref(&rectangle.start(), PointSpace::History),
                "resolve Ghostty image anchor",
            )?
            else {
                continue;
            };
            let source = check(placement.source_rect(&image), "read Ghostty image crop")?;
            let pixels = check(
                placement.pixel_size(&image, terminal),
                "read Ghostty image size",
            )?;
            retained.insert(id);
            output.placements.push(Placement {
                image: id,
                id: check(placement.placement_id(), "read Ghostty placement ID")?,
                column: point.x as i32,
                row: (i64::from(point.y) - history) as i32,
                offset: [
                    check(placement.x_offset(), "read Ghostty image x offset")?,
                    check(placement.y_offset(), "read Ghostty image y offset")?,
                ],
                size: [pixels.width, pixels.height],
                source: [source.x, source.y, source.width, source.height],
                tile: None,
                cell: [
                    viewport.pixel_width / u32::from(viewport.columns),
                    viewport.pixel_height / u32::from(viewport.rows),
                ],
                z: check(placement.z(), "read Ghostty image layer")?,
            });
        }
        if !virtuals.is_empty() {
            let placements = unicode::project(terminal, viewport, history as usize, &virtuals)?;
            retained.extend(placements.iter().map(|placement| placement.image));
            output.placements.extend(placements);
        }
        self.images.retain(|id, _| retained.contains(id));
        output.images = self.images.values().cloned().collect();
        output
            .placements
            .sort_by_key(|placement| (placement.z, placement.image, placement.id));
        Ok(output)
    }
}
