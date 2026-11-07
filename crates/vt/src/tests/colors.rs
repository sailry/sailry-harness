use super::*;

fn theme(scheme: ColorScheme) -> Appearance {
    let mut value = appearance(scheme);
    if scheme == ColorScheme::Light {
        std::mem::swap(&mut value.foreground, &mut value.background);
    }
    value.palette[0] = Rgb {
        red: 0,
        green: 0,
        blue: 0,
    };
    value.palette[7] = Rgb {
        red: 224,
        green: 224,
        blue: 224,
    };
    value.palette[15] = Rgb {
        red: 255,
        green: 255,
        blue: 255,
    };
    value
}

fn cell(screen: &Screen, column: u16) -> TextStyle {
    screen.rows[0]
        .spans
        .iter()
        .find(|span| span.column <= column && column < span.column + span.columns)
        .unwrap()
        .style
}

fn report(target: &str, color: Rgb) -> Vec<u8> {
    format!(
        "\x1b]{target};rgb:{:04x}/{:04x}/{:04x}\x1b\\",
        u16::from(color.red) * 257,
        u16::from(color.green) * 257,
        u16::from(color.blue) * 257,
    )
    .into_bytes()
}

#[test]
fn indexed_colors_run_dark_to_light() {
    for scheme in [ColorScheme::Light, ColorScheme::Dark] {
        let appearance = theme(scheme);
        let mut terminal = Vt::new(&viewport(20, 4), &appearance, 20).unwrap();
        terminal
            .write(b"\x1b[48;5;16mA\x1b[48;5;231mB\x1b[48;5;232mC\x1b[48;5;255mD\x1b[0m")
            .unwrap();
        let screen = terminal.checkpoint();
        let dark = cell(&screen, 0).background.unwrap();
        let light = cell(&screen, 1).background.unwrap();
        let low = cell(&screen, 2).background.unwrap();
        let high = cell(&screen, 3).background.unwrap();
        for (dark, light, low, high) in [
            (dark.red, light.red, low.red, high.red),
            (dark.green, light.green, low.green, high.green),
            (dark.blue, light.blue, low.blue, high.blue),
        ] {
            assert!(dark < low && low < high && high < light, "{scheme:?}");
        }
        assert_eq!(screen.foreground, appearance.foreground);
        assert_eq!(screen.background, appearance.background);
    }
}

#[test]
fn named_colors_stay_separate_from_defaults() {
    for scheme in [ColorScheme::Light, ColorScheme::Dark] {
        let appearance = theme(scheme);
        let mut terminal = Vt::new(&viewport(20, 4), &appearance, 20).unwrap();
        terminal
            .write(b"\x1b[30mA\x1b[37mB\x1b[97mC\x1b[0mD\x1b[47mE\x1b[107mF\x1b[0mG")
            .unwrap();
        let screen = terminal.checkpoint();
        for (column, index) in [(0, 0), (1, 7), (2, 15)] {
            assert_eq!(
                cell(&screen, column).foreground,
                Some(appearance.palette[index])
            );
        }
        assert_eq!(cell(&screen, 3), TextStyle::default());
        assert_eq!(cell(&screen, 4).background, Some(appearance.palette[7]));
        assert_eq!(cell(&screen, 5).background, Some(appearance.palette[15]));
        assert_eq!(cell(&screen, 6), TextStyle::default());
    }
}

#[test]
fn explicit_rgb_survives_theme_updates() {
    let mut terminal = Vt::new(&viewport(20, 4), &theme(ColorScheme::Dark), 20).unwrap();
    terminal
        .write(b"\x1b[38;2;12;34;56;48;2;210;220;230mR\x1b[0mD")
        .unwrap();
    for scheme in [ColorScheme::Dark, ColorScheme::Light] {
        let appearance = theme(scheme);
        terminal.appearance(&appearance).unwrap();
        let screen = terminal.checkpoint();
        assert_eq!(
            cell(&screen, 0).foreground,
            Some(Rgb {
                red: 12,
                green: 34,
                blue: 56
            })
        );
        assert_eq!(
            cell(&screen, 0).background,
            Some(Rgb {
                red: 210,
                green: 220,
                blue: 230
            })
        );
        assert_eq!(cell(&screen, 1), TextStyle::default());
        assert_eq!(screen.foreground, appearance.foreground);
        assert_eq!(screen.background, appearance.background);
    }
}

#[test]
fn queries_follow_overrides_and_resets() {
    let dark = theme(ColorScheme::Dark);
    let light = theme(ColorScheme::Light);
    let mut terminal = Vt::new(&viewport(20, 4), &dark, 20).unwrap();
    let query = b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b]4;1;?\x1b\\";
    let expected = |appearance: &Appearance| {
        [
            report("10", appearance.foreground),
            report("11", appearance.background),
            report("4;1", appearance.palette[1]),
        ]
        .concat()
    };
    assert_eq!(
        terminal.write(query).unwrap().responses.concat(),
        expected(&dark)
    );
    terminal
        .write(b"\x1b]10;#112233\x1b\\\x1b]11;#445566\x1b\\\x1b]4;1;#778899\x1b\\\x1b[?2031h")
        .unwrap();
    let overrides = terminal.write(query).unwrap().responses.concat();
    assert_eq!(
        overrides,
        [
            report(
                "10",
                Rgb {
                    red: 0x11,
                    green: 0x22,
                    blue: 0x33
                }
            ),
            report(
                "11",
                Rgb {
                    red: 0x44,
                    green: 0x55,
                    blue: 0x66
                }
            ),
            report(
                "4;1",
                Rgb {
                    red: 0x77,
                    green: 0x88,
                    blue: 0x99
                }
            ),
        ]
        .concat()
    );
    let update = terminal.appearance(&light).unwrap();
    assert_eq!(update.responses, vec![b"\x1b[?997;2n".to_vec()]);
    assert_eq!(
        terminal.checkpoint().background,
        Rgb {
            red: 0x44,
            green: 0x55,
            blue: 0x66
        }
    );
    assert_eq!(terminal.write(query).unwrap().responses.concat(), overrides);
    terminal
        .write(b"\x1b]110\x1b\\\x1b]111\x1b\\\x1b]104;1\x1b\\")
        .unwrap();
    assert_eq!(
        terminal.write(query).unwrap().responses.concat(),
        expected(&light)
    );
    assert_eq!(terminal.checkpoint().background, light.background);
    assert_eq!(
        terminal.write(b"\x1b[?996n").unwrap().responses,
        vec![b"\x1b[?997;2n".to_vec()]
    );
    terminal.write(b"\x1b[?2031l").unwrap();
    assert!(terminal.appearance(&dark).unwrap().responses.is_empty());
    assert_eq!(
        terminal.write(b"\x1b[?996n").unwrap().responses,
        vec![b"\x1b[?997;1n".to_vec()]
    );
}
