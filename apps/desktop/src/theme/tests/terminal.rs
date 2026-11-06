use super::*;
use sailry_protocol::terminal::{ColorScheme, Rgb};

fn color(value: Rgb) -> Hsla {
    gpui_kit::rgb(
        (u32::from(value.red) << 16) | (u32::from(value.green) << 8) | u32::from(value.blue),
    )
    .into()
}

#[gpui::test]
fn named_neutrals_ignore_default_colors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            let initial = super::super::terminal(cx);
            assert_eq!(
                initial.palette[0],
                Rgb {
                    red: 0,
                    green: 0,
                    blue: 0
                }
            );
            assert_eq!(
                initial.palette[15],
                Rgb {
                    red: 255,
                    green: 255,
                    blue: 255
                }
            );
            assert!(initial.palette[7].red > 200);
            assert!(initial.palette[7].red < initial.palette[15].red);
            Theme::update(cx, |theme| {
                theme.foreground = gpui_kit::rgb(0x243546).into();
                theme.background = gpui_kit::rgb(0xe1e8ef).into();
            });
            let changed = super::super::terminal(cx);
            assert_ne!(changed.foreground, initial.foreground);
            assert_ne!(changed.background, initial.background);
            for index in [0, 7, 15] {
                assert_eq!(changed.palette[index], initial.palette[index]);
            }
        }
    });
}

#[gpui::test]
fn light_foregrounds_keep_contrast(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
        Theme::change(ThemeMode::Light, None, cx);
        let appearance = super::super::terminal(cx);
        assert_eq!(appearance.color_scheme, ColorScheme::Light);
        for index in [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14] {
            let ratio = contrast(
                color(appearance.palette[index]),
                color(appearance.background),
            );
            assert!(ratio >= 4.5, "ANSI index {index} has contrast {ratio}");
        }
    });
}

#[gpui::test]
fn dark_hues_retain_theme_values(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let appearance = super::super::terminal(cx);
        assert_eq!(appearance.color_scheme, ColorScheme::Dark);
        let theme = cx.theme();
        for (index, expected) in [
            (1, theme.red),
            (2, theme.green),
            (3, theme.yellow),
            (4, theme.blue),
            (5, theme.magenta),
            (6, theme.cyan),
            (9, theme.red_light),
            (10, theme.green_light),
            (11, theme.yellow_light),
            (12, theme.blue_light),
            (13, theme.magenta_light),
            (14, theme.cyan_light),
        ] {
            let expected = theme.background.blend(expected).to_rgb();
            assert_eq!(
                appearance.palette[index],
                Rgb {
                    red: (expected.r * 255.).round() as u8,
                    green: (expected.g * 255.).round() as u8,
                    blue: (expected.b * 255.).round() as u8,
                }
            );
        }
    });
}
