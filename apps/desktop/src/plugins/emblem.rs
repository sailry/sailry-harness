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

/// Marketplace identity artwork leaves navigation's existing palette unchanged.
pub(crate) fn grid(
    name: &str,
    tint: Option<sailry_protocol::plugin::Tint>,
    glyph: Option<&Glyph>,
    data: Option<&str>,
    size: Pixels,
) -> AnyElement {
    let icon_size = if size <= px(40.) { px(22.) } else { px(28.) };
    let icon = if let Some((format, bytes)) = image_source(None, data) {
        img(std::sync::Arc::new(Image::from_bytes(format, bytes)))
            .size(icon_size)
            .into_any_element()
    } else {
        glyph
            .map(crate::assets::icons::icon)
            .unwrap_or_else(|| Icon::default().path("reicon:ui/puzzle-piece"))
            .size(icon_size)
            .text_color(gpui_kit::white())
            .into_any_element()
    };
    surface(name, tint, size).child(icon).into_any_element()
}

/// The gradient grid tile shared by identity artwork; callers supply the white glyph.
pub(crate) fn surface(
    name: &str,
    tint: Option<sailry_protocol::plugin::Tint>,
    size: Pixels,
) -> Div {
    let (top, base) = gradient(tint.unwrap_or_else(|| fallback(name)));
    h_flex()
        .relative()
        .size(size)
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(if size <= px(40.) { px(11.) } else { px(14.) })
        .overflow_hidden()
        .bg(gpui_kit::linear_gradient(
            180.,
            gpui_kit::linear_color_stop(rgb(top), 0.),
            gpui_kit::linear_color_stop(rgb(base), 1.),
        ))
        .children((1..4).flat_map(|index| {
            let position = size * (index as f32 / 4.);
            [
                gpui_kit::div()
                    .absolute()
                    .left(position)
                    .top_0()
                    .w(px(1.))
                    .h_full()
                    .bg(gpui_kit::hsla(0., 0., 1., 0.1))
                    .into_any_element(),
                gpui_kit::div()
                    .absolute()
                    .top(position)
                    .left_0()
                    .h(px(1.))
                    .w_full()
                    .bg(gpui_kit::hsla(0., 0., 1., 0.1))
                    .into_any_element(),
            ]
        }))
}

fn fallback(name: &str) -> sailry_protocol::plugin::Tint {
    use sailry_protocol::plugin::Tint;
    let hash = name.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    [
        Tint::Blue,
        Tint::Violet,
        Tint::Teal,
        Tint::Amber,
        Tint::Gray,
    ][hash as usize % 5]
}

fn gradient(tint: sailry_protocol::plugin::Tint) -> (u32, u32) {
    use sailry_protocol::plugin::Tint;
    match tint {
        Tint::Blue => (0x45b8ff, 0x1d6ef0),
        Tint::Violet => (0xa98bff, 0x6440dc),
        Tint::Teal => (0x3fd3b4, 0x0f8c7c),
        Tint::Amber => (0xffc65c, 0xe07a18),
        Tint::Gray => (0xa2a7b2, 0x565b66),
    }
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
    fn fallback_tints_are_stable() {
        use sailry_protocol::plugin::Tint;
        // Lock the algorithm so refactors cannot recolor existing tiles.
        assert_eq!(fallback("office"), Tint::Teal);
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
