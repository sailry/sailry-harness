// Modified by Longbridge for gpui-fast.
//! Native fonts made once per size and kept, so CoreText's shaping caches survive from line to line.

use collections::HashMap;
use core_text::font::CTFont;
use font_kit::font::Font as FontKitFont;
use gpui::{FontId, Pixels};

/// Each font at each size a line has been laid out at. CoreText keeps the
/// caches it builds for shaping — the advances and glyphs of ASCII among
/// them — on the font object, so a font made afresh for every line built
/// them again for every line.
#[derive(Default)]
pub(crate) struct SizedFonts(HashMap<(FontId, u32), CTFont>);

impl SizedFonts {
    /// The native font for `font`, whose id is `font_id`, at `font_size`, made
    /// once and kept.
    pub(crate) fn get(&mut self, font_id: FontId, font: &FontKitFont, font_size: Pixels) -> CTFont {
        // Sizes can animate, and every size a font is drawn at would otherwise
        // be kept for good.
        const MAX_SIZED_FONTS: usize = 1024;
        let key = (font_id, f32::from(font_size).to_bits());
        if let Some(font) = self.0.get(&key) {
            return font.clone();
        }
        if self.0.len() >= MAX_SIZED_FONTS {
            self.0.clear();
        }
        let font = font
            .native_font()
            .clone_with_font_size(f32::from(font_size).into());
        self.0.insert(key, font.clone());
        font
    }
}

#[cfg(test)]
mod tests {
    use crate::MacTextSystem;
    use gpui::{FontRun, PlatformTextSystem, font, px};

    /// Fonts are made once per size and kept, so a line laid out with a kept
    /// font has to come out exactly as it did with a new one: the same glyphs
    /// in the same places, however many runs split it into sizes a hair
    /// apart, and never mixed up with the same font at another size.
    #[test]
    fn lines_laid_out_with_kept_fonts_match_the_first_layout() {
        let fonts = MacTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();
        let line = "AVAWAY fi ffi 12.5 office";
        // Runs alternate between two sizes a hair apart, to keep ligatures
        // from joining across them.
        let runs = [5, 3, 4, 5, 8].map(|len| FontRun { font_id, len });
        let shape = |size| {
            let layout = fonts.layout_line(line, px(size), &runs);
            let glyphs = layout
                .runs
                .iter()
                .flat_map(|run| run.glyphs.iter().map(|glyph| (glyph.id, glyph.position)))
                .collect::<Vec<_>>();
            (layout.width, glyphs)
        };

        let first = shape(16.);
        assert_eq!(shape(16.), first, "a kept font shapes differently");
        let larger = shape(24.);
        assert_ne!(larger.0, first.0, "another size has to be another font");
        assert_eq!(shape(16.), first, "a size laid out again after another");
        assert_eq!(shape(24.), larger);
    }
}
