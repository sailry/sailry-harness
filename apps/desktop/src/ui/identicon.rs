//! Stable procedural agent badges, rendered by Kit's ordinary SVG Icon.
use gpui_kit::{component::*, *};
use std::{fmt::Write, sync::LazyLock, time::Duration};

fn identity(key: &str) -> (usize, usize) {
    // Fixed hashing keeps a session's identity unchanged across platforms and launches.
    let hash = key.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    ((hash % 8) as usize, ((hash >> 16) % 8) as usize)
}

fn designs() -> &'static [String; 8] {
    static DESIGNS: LazyLock<[String; 8]> = LazyLock::new(|| {
        std::array::from_fn(|shape| {
            let mut svg = String::from(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='black'>",
            );
            let count = [6, 4, 8, 8, 5, 6, 4, 10][shape];
            for index in 0..count {
                let angle = index as f32 * 360. / count as f32;
                write!(svg, "<g transform='rotate({angle} 12 12)'>").unwrap();
                svg.push_str(match shape {
                    0 => "<ellipse cx='12' cy='6.8' rx='3.1' ry='4.5' opacity='.45'/><ellipse cx='12' cy='6.3' rx='1.5' ry='2.3' opacity='.5'/>",
                    1 => "<circle cx='12' cy='6.5' r='4.5' opacity='.45'/><circle cx='12' cy='5.6' r='2' opacity='.42'/>",
                    2 => "<rect x='10.6' y='1.5' width='2.8' height='7.2' rx='1.4' opacity='.7'/>",
                    3 => "<path d='M12 2 Q16 9 12 12 Q8 9 12 2' opacity='.42'/>",
                    4 => "<ellipse cx='12' cy='6.5' rx='3.7' ry='4.8' opacity='.5'/>",
                    5 => "<circle cx='12' cy='5.5' r='2.6' opacity='.6'/><circle cx='12' cy='9' r='1.4' opacity='.42'/>",
                    6 => "<path d='M12 2 L15 8 L12 12 L9 8 Z' opacity='.6'/><circle cx='12' cy='5' r='1.3' opacity='.45'/>",
                    _ => "<ellipse cx='12' cy='6.5' rx='1.4' ry='4.8' opacity='.5'/>",
                });
                svg.push_str("</g>");
            }
            svg.push_str("<circle cx='12' cy='12' r='2.3' opacity='.85'/></svg>");
            svg
        })
    });
    &DESIGNS
}

pub(crate) fn agent(key: &str, cx: &App) -> Icon {
    child(key, true, cx)
}

pub(crate) fn child(key: &str, running: bool, cx: &App) -> Icon {
    let (shape, color) = identity(key);
    let theme = cx.theme();
    Icon::default()
        .data(designs()[shape].as_bytes())
        .size_4()
        .flex_shrink_0()
        .text_color(tint(
            color,
            running,
            theme.is_dark(),
            theme.muted_foreground,
        ))
}

fn tint(index: usize, running: bool, dark: bool, muted: Hsla) -> Hsla {
    if !running {
        return muted;
    }
    let colors = [
        ColorName::Cyan,
        ColorName::Blue,
        ColorName::Pink,
        ColorName::Amber,
        ColorName::Teal,
        ColorName::Violet,
        ColorName::Orange,
        ColorName::Indigo,
    ];
    colors[index].scale(if dark { 400 } else { 600 })
}

pub(crate) fn rotation() -> Animation {
    // Kit's Spinner fixes its speed at 0.8 seconds; use its Icon animation API
    // for a 450 ms revolution followed by a 550 ms pause, once per second.
    Animation::new(Duration::from_secs(1))
        .repeat()
        .with_easing(|phase| {
            let progress = (phase / 0.45).min(1.);
            progress * progress * (3. - 2. * progress)
        })
}

pub(crate) fn animated(icon: Icon, running: bool) -> AnyElement {
    if running {
        icon.with_animation("session-rotation", rotation(), |icon, phase| {
            icon.transform(Transformation::rotate(percentage(phase)))
        })
        .into_any_element()
    } else {
        icon.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn identities_are_stable_and_varied() {
        let identities: std::collections::HashSet<_> = (0..256)
            .map(|id| identity(&format!("agent-{id}")))
            .collect();
        assert!(identities.len() > 32);
        assert_eq!(identity("agent-7"), (2, 0));
        assert_ne!(identity("agent-7"), identity("agent-8"));
    }

    #[test]
    fn settled_badges_use_muted_color() {
        let muted = hsla(0., 0., 0.5, 1.);
        for index in 0..8 {
            assert_eq!(tint(index, false, false, muted), muted);
            assert_eq!(tint(index, false, true, muted), muted);
        }
    }

    #[test]
    fn running_badges_retain_identity_color() {
        let muted = hsla(0., 0., 0.5, 1.);
        assert_eq!(tint(0, true, false, muted), ColorName::Cyan.scale(600));
        assert_eq!(tint(0, true, true, muted), ColorName::Cyan.scale(400));
        assert_ne!(tint(0, true, true, muted), tint(1, true, true, muted));
    }

    #[gpui_kit::test]
    fn all_badges_render(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let renderer = cx.svg_renderer();
            for source in designs() {
                let document = renderer.parse_svg(source.as_bytes()).unwrap();
                let image = renderer
                    .render_parsed(
                        &document,
                        SvgSize::ExactSize(size(DevicePixels(32), DevicePixels(32))),
                    )
                    .unwrap();
                assert_eq!(image.size(0), size(DevicePixels(32), DevicePixels(32)));
            }
        });
    }
}
