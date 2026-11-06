//! Bounded image thumbnails of preview artifacts. Message rows show images,
//! never live views; the controller-side cache can always be rebuilt.
// Placement and cropping serve the native renderer; Linux has none.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
use crate::theme::css::Palette;
use gpui_kit::{App, Global, Task, Window};
use sailry_protocol::{
    ArtifactId, NodeId,
    artifact::{Kind, Summary},
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod surface;
#[cfg(test)]
mod tests;

pub(crate) const MIN_WIDTH: f32 = 240.;
pub(crate) const MAX_WIDTH: f32 = 960.;
const STEP: f32 = 80.;
pub(crate) const MIN_HEIGHT: f32 = 96.;
pub(crate) const MAX_HEIGHT: f32 = 360.;
const PADDING: f32 = 24.;
/// Authored layouts see this viewport, so `100vh` content stays bounded.
pub(crate) const VIEWPORT: f32 = MAX_HEIGHT + 2. * PADDING;
const CAPACITY: usize = 48;
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Documents keep their card; previews get a thumbnail where a native renderer exists.
pub(crate) fn eligible(artifact: &Summary) -> bool {
    SUPPORTED && artifact.kind == Kind::Preview && artifact.valid()
}

/// Rounds a card width up to a shared rendering width.
pub(crate) fn bucket(width: f32) -> u32 {
    if !width.is_finite() {
        return MIN_WIDTH as u32;
    }
    ((width / STEP).ceil() * STEP).clamp(MIN_WIDTH, MAX_WIDTH) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub(crate) node: NodeId,
    pub(crate) artifact: ArtifactId,
    pub(crate) revision: u32,
}

impl Target {
    pub(crate) fn new(node: NodeId, artifact: &Summary) -> Self {
        Self {
            node,
            artifact: artifact.id,
            revision: artifact.revision,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) target: Target,
    pub(crate) theme: u64,
    pub(crate) width: u32,
    pub(crate) scale: u32,
}

impl Key {
    pub(crate) fn new(target: Target, theme: u64, width: f32, scale: f32) -> Self {
        let scale = if scale.is_finite() { scale } else { 1. };
        Self {
            target,
            theme,
            width: bucket(width),
            scale: (scale.clamp(0.5, 4.) * 100.).round() as u32,
        }
    }
}

/// The authored content box reported from inside the sandboxed frame.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub(crate) struct Measure {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) scroll_height: f32,
    pub(crate) opaque: bool,
}

/// Thumbnail height and the frame offset that centers authored content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub(crate) height: f32,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

/// Opaque pages keep their own layout from the top. Transparent content is
/// centered on the dotted wrapper, or top-aligned when taller than the bound.
pub(crate) fn place(measure: &Measure, width: f32) -> Option<Placement> {
    let values = [
        measure.left,
        measure.top,
        measure.width,
        measure.height,
        measure.scroll_height,
        width,
    ];
    if values.iter().any(|value| !value.is_finite()) || width < 1. || measure.top >= VIEWPORT {
        return None;
    }
    if measure.opaque {
        let height = measure
            .scroll_height
            .max(measure.top + measure.height)
            .clamp(MIN_HEIGHT, MAX_HEIGHT);
        return Some(Placement {
            height: height.round(),
            x: 0.,
            y: 0.,
        });
    }
    if measure.width < 1. || measure.height < 1. || measure.left >= width {
        return None;
    }
    let height = (measure.height + 2. * PADDING).clamp(MIN_HEIGHT, MAX_HEIGHT);
    let y = if measure.height + 2. * PADDING <= MAX_HEIGHT {
        (height - measure.height) / 2. - measure.top
    } else {
        PADDING - measure.top
    };
    let x = (width - measure.width) / 2. - measure.left;
    Some(Placement {
        height: height.round(),
        x: x.clamp(-4. * MAX_WIDTH, MAX_WIDTH).round(),
        y: y.clamp(-VIEWPORT, MAX_HEIGHT).round(),
    })
}

/// Crops a native capture of the `width`-point page to the placed height and
/// re-encodes it as PNG. A uniform frame was never painted.
pub(crate) fn crop(
    bytes: &[u8],
    format: image::ImageFormat,
    width: f32,
    placement: Placement,
) -> Result<Vec<u8>, String> {
    let captured = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| "Snapshot capture is not a valid image".to_owned())?;
    if captured.width() == 0 || captured.height() == 0 || width < 1. {
        return Err("Snapshot capture is empty".into());
    }
    let scale = captured.width() as f32 / width;
    let height = ((placement.height * scale).round() as u32).clamp(1, captured.height());
    let pixels = captured.crop_imm(0, 0, captured.width(), height).to_rgba8();
    let first = *pixels.get_pixel(0, 0);
    if pixels.pixels().all(|pixel| *pixel == first) {
        return Err("Snapshot capture is blank".into());
    }
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|_| "Snapshot could not be encoded".to_owned())?;
    let encoded = encoded.into_inner();
    if encoded.len() > MAX_PNG_BYTES {
        return Err("Snapshot exceeds the supported size".into());
    }
    Ok(encoded)
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) image: Arc<gpui_kit::Image>,
    /// Logical size at the rendering width; cards scale it to fit.
    pub(crate) width: f32,
    pub(crate) height: f32,
}

enum Entry {
    Pending,
    Ready { snapshot: Snapshot, stamp: u64 },
    Failed { stamp: u64 },
}

/// Prepared thumbnails for every conversation view. Render reads only;
/// changes happen outside layout so they cannot schedule another frame.
#[derive(Default)]
pub(crate) struct Cache {
    entries: HashMap<Key, Entry>,
    latest: HashMap<Target, Key>,
    clock: u64,
}

impl Global for Cache {}

impl Cache {
    pub(crate) fn wanted(&self, key: &Key) -> bool {
        !self.entries.contains_key(key)
    }

    pub(crate) fn begin(&mut self, key: Key) {
        self.entries.insert(key, Entry::Pending);
        self.latest.insert(key.target, key);
    }

    pub(crate) fn finish(&mut self, key: Key, result: Result<Snapshot, String>) {
        self.clock += 1;
        let stamp = self.clock;
        self.entries.insert(
            key,
            match result {
                Ok(snapshot) => Entry::Ready { snapshot, stamp },
                Err(_) => Entry::Failed { stamp },
            },
        );
        self.evict();
    }

    /// What a card shows: the current theme's latest image, else another image
    /// of the same revision while that one renders. A failed current request
    /// keeps the ordinary card.
    pub(crate) fn show(&self, target: &Target, theme: u64) -> Option<Snapshot> {
        let latest = self.latest.get(target);
        if latest.is_some_and(|key| {
            key.theme == theme && matches!(self.entries.get(key), Some(Entry::Failed { .. }))
        }) {
            return None;
        }
        self.entries
            .iter()
            .filter_map(|(key, entry)| match entry {
                Entry::Ready { snapshot, stamp } if key.target == *target => {
                    Some(((key.theme == theme, Some(key) == latest, *stamp), snapshot))
                }
                _ => None,
            })
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, snapshot)| snapshot.clone())
    }

    fn evict(&mut self) {
        loop {
            let settled = self
                .entries
                .iter()
                .filter_map(|(key, entry)| match entry {
                    Entry::Ready { stamp, .. } | Entry::Failed { stamp } => Some((*stamp, *key)),
                    Entry::Pending => None,
                })
                .collect::<Vec<_>>();
            if settled.len() <= CAPACITY {
                break;
            }
            let Some((_, oldest)) = settled.into_iter().min_by_key(|(stamp, _)| *stamp) else {
                break;
            };
            self.entries.remove(&oldest);
        }
        let entries = &self.entries;
        self.latest
            .retain(|target, _| entries.keys().any(|key| key.target == *target));
    }
}

/// Renders authored preview HTML at `width` on the shared hidden surface.
pub(crate) fn render(
    html: String,
    palette: Palette,
    width: u32,
    window: &mut Window,
    cx: &mut App,
) -> Task<Result<Snapshot, String>> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        surface::render(
            surface::Request {
                html,
                palette,
                width: width as f32,
            },
            window,
            cx,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (html, palette, width, window, cx);
        Task::ready(Err("Preview thumbnails are unavailable".into()))
    }
}

pub(super) fn init(cx: &mut App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    surface::init(cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cx;
}

pub(super) fn release(window: &mut Window, cx: &mut App) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    surface::release(window, cx);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, cx);
}
