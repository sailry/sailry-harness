//! Plugin identity uses the shared glyph catalog and a stable name-derived color.
use gpui_kit::{
    AnyElement, App, Div, Hsla, Image, ImageFormat, IntoElement, ParentElement, Pixels, Styled,
    component::{ActiveTheme, Icon, h_flex},
    img,
    prelude::FluentBuilder as _,
    px, rgb,
};
use sailry_protocol::plugin::{Info, desktop::Icon as Glyph};

pub(crate) fn glyph(info: &Info) -> Option<&Glyph> {
    let extension = info.extension.as_ref()?;
    extension
        .display
        .as_ref()
        .and_then(|display| display.icon.as_ref())
        .or_else(|| {
            extension
                .desktop
                .as_ref()?
                .navigation
                .as_ref()?
                .icon
                .as_ref()
        })
}

fn color(name: &str) -> Hsla {
    // Identity colors deliberately exclude muddy hues from an unrestricted hue sweep.
    const COLORS: [u32; 8] = [
        0x3D80F5, 0x28B49C, 0x4398DF, 0x9275DF, 0x626D80, 0xE86E83, 0x38A6B8, 0x647DE8,
    ];
    // FNV-1a is explicit: DefaultHasher does not promise stable cross-version output.
    // Use package identity, never the localized title, ordering or enabled state.
    let hash = name.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    rgb(COLORS[hash as usize % COLORS.len()]).into()
}

fn tint(name: &str, cx: &App) -> Hsla {
    let color = color(name);
    if cx.theme().is_dark() {
        color.blend(cx.theme().foreground.opacity(0.35))
    } else {
        color
    }
}

fn image_source(glyph: Option<&Glyph>, data: Option<&str>) -> Option<(ImageFormat, Vec<u8>)> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    if let Some(bytes) = data.and_then(|data| STANDARD.decode(data).ok()) {
        return Some((ImageFormat::Png, bytes));
    }
    if let Some(Glyph::Svg { svg }) = glyph {
        return Some((ImageFormat::Svg, svg.as_bytes().to_vec()));
    }
    None
}

pub(crate) fn render(
    name: &str,
    glyph: Option<&Glyph>,
    data: Option<&str>,
    size: Pixels,
    cx: &App,
) -> AnyElement {
    let tile = tile(size, cx);
    if let Some((format, bytes)) = image_source(glyph, data) {
        let image = std::sync::Arc::new(Image::from_bytes(format, bytes));
        return tile.child(img(image).size_full()).into_any_element();
    }
    let icon = glyph
        .map(crate::assets::icons::icon)
        .unwrap_or_else(|| Icon::default().path("reicon:ui/puzzle-piece"));
    tile.child(icon.size(size * 0.6).text_color(tint(name, cx)))
        .into_any_element()
}

fn tile(size: Pixels, cx: &App) -> Div {
    h_flex()
        .size(size)
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(if size <= px(20.) {
            cx.theme().radius
        } else {
            cx.theme().radius_2xl()
        })
        .overflow_hidden()
        .when(cx.theme().is_dark(), |tile| tile.bg(cx.theme().muted))
        .when(!cx.theme().is_dark(), |tile| {
            tile.border_1().border_color(cx.theme().border)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_stable() {
        // Lock the algorithm so refactors cannot recolor existing identities.
        assert_eq!(color("office"), rgb(0x28B49C).into());
        assert_ne!(color("office"), color("project-summary"));
    }

    #[gpui_kit::test]
    fn follows_theme(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::{Theme, ThemeMode};
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
                Theme::change(mode, None, cx);
                if mode.is_dark() {
                    assert_eq!(
                        tint("office", cx),
                        color("office").blend(cx.theme().foreground.opacity(0.35))
                    );
                    assert!(tint("office", cx).s < color("office").s);
                    assert!(tint("office", cx).l > color("office").l);
                } else {
                    assert_eq!(tint("office", cx), color("office"));
                }
                for size in [px(20.), px(48.)] {
                    let mut tile = tile(size, cx);
                    let style = tile.style();
                    if mode.is_dark() {
                        assert_eq!(style.background, Some(cx.theme().muted.into()));
                        assert_eq!(style.border_widths.top, None);
                    } else {
                        assert_eq!(style.background, None);
                        assert_eq!(style.border_widths.top, Some(px(1.).into()));
                        assert_eq!(style.border_color, Some(cx.theme().border));
                    }
                }
            }
        });
    }

    #[test]
    fn preserves_custom_images() {
        let glyph = Glyph::Name("reicon:ui/puzzle-piece".into());
        assert_eq!(
            image_source(Some(&glyph), Some("aWNvbg==")),
            Some((ImageFormat::Png, b"icon".to_vec()))
        );
        let glyph = Glyph::Svg {
            svg: "<svg fill=\"red\"/>".into(),
        };
        assert_eq!(
            image_source(Some(&glyph), None),
            Some((ImageFormat::Svg, b"<svg fill=\"red\"/>".to_vec()))
        );
        assert!(image_source(None, Some("invalid base64")).is_none());
    }
}
