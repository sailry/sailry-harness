use super::*;
use core::prelude::v1::test;

fn catalog(loaded: &Loaded) -> Catalog {
    let mut builtin = Package::preview(loaded);
    builtin.id = "builtin".into();
    builtin.manifest = None;
    builtin.images.clear();
    Catalog {
        packages: vec![builtin, Package::preview(loaded)],
        selected: 1,
        root: None,
        error: None,
    }
}

#[test]
fn replaces_in_place_at_package_limit() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut loaded = Loaded::read(&source, true).unwrap();
    let mut catalog = catalog(&loaded);
    for index in 0..31 {
        let mut package = Package::preview(&loaded);
        package.id = format!("other-{index}");
        catalog.packages.push(package);
    }
    loaded.manifest.name = "Updated Tide".into();
    catalog.add(loaded).unwrap();
    assert_eq!(catalog.packages.len(), 33);
    assert_eq!(catalog.selected, 1);
    assert_eq!(catalog.packages[1].name.as_ref(), "Updated Tide");
    assert_eq!(catalog.packages[2].id, "other-0");
    let mut loaded = Loaded::read(&source, true).unwrap();
    loaded.manifest.id = "new-theme".into();
    assert!(catalog.add(loaded).is_err());
    assert_eq!(catalog.packages.len(), 33);
}

#[test]
fn accounts_for_replaced_images() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut loaded = Loaded::read(&source, true).unwrap();
    let image = Arc::new(RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::new(4096, 2048),
    )]));
    loaded.images = BTreeMap::from([("assets/large.png".into(), image)]);
    let mut catalog = catalog(&loaded);
    for id in ["shore", "river", "lake"] {
        let mut package = Package::preview(&loaded);
        package.id = id.into();
        catalog.packages.push(package);
    }
    // The catalog is at 128 MiB. Replacing its 32 MiB package must not double-count it.
    catalog.check(&loaded).unwrap();
    loaded.manifest.id = "new-theme".into();
    assert!(catalog.check(&loaded).is_err());
    loaded.manifest.id = "tide".into();
    loaded.images.insert(
        "assets/extra.png".into(),
        loaded.images["assets/large.png"].clone(),
    );
    assert!(catalog.add(loaded).is_err());
    assert_eq!(catalog.packages.len(), 5);
    assert_eq!(catalog.packages[1].images.len(), 1);
    assert_eq!(catalog.selected, 1);
}

#[gpui::test]
fn normalizes_floating_alpha(cx: &mut TestAppContext) {
    use crate::theme::DialogStyle as _;

    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "surfaces");
    let mut package = Package::preview(&Loaded::read(&source, true).unwrap());
    for (config, popover) in [
        (&mut package.light, "#12345620"),
        (
            &mut package.dark,
            "linear-gradient(90deg, #abcdef, #fedcba00)",
        ),
    ] {
        let colors = &mut Rc::make_mut(config).colors;
        colors.popover = Some(popover.into());
        colors.background = Some("#10203080".into());
        colors.overlay = Some("#00000077".into());
        colors.list_hover = Some("#22334455".into());
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let visual = cx.add_empty_window();
    for (mode, expected) in [
        (ThemeMode::Light, "#123456"),
        (ThemeMode::Dark, "#abcdef"),
        (ThemeMode::Light, "#123456"),
    ] {
        visual.update(|window, cx| {
            crate::theme::select(Some(mode), window, cx);
            apply_colors(package.clone(), Some(window), cx);
            let theme = cx.theme();
            let expected = Hsla::parse_hex(expected)
                .unwrap()
                .alpha(crate::theme::FLOATING_ALPHA);
            assert_eq!(theme.popover, expected);
            assert_eq!(theme.tokens.popover.color, expected);
            assert_eq!(theme.tokens.popover.background.as_solid(), Some(expected));
            assert_eq!(theme.background, Hsla::parse_hex("#10203080").unwrap());
            assert_eq!(theme.overlay, Hsla::parse_hex("#00000077").unwrap());
            assert_eq!(theme.list_hover, Hsla::parse_hex("#22334455").unwrap());
            assert!(crate::theme::panel_background(cx).a < 1.);
            let mut dialog = gpui_kit::component::dialog::Dialog::new(cx).floating_surface(cx);
            assert_eq!(dialog.style().background, Some(expected.into()));
        });
    }
}

mod slider {
    use super::*;

    fn package(directory: &std::path::Path) -> Package {
        let source = crate::theme::fixture::write(directory, "colors");
        let mut package = Package::preview(&Loaded::read(&source, true).unwrap());
        for (config, primary, foreground) in [
            (&mut package.light, "#b58128", "#18120a"),
            (&mut package.dark, "#ddbc70", "#241e12"),
        ] {
            let colors = &mut Rc::make_mut(config).colors;
            colors.primary = Some(primary.into());
            colors.primary_foreground = Some(foreground.into());
            colors.slider_bar = None;
            colors.slider_thumb = None;
        }
        package
    }

    #[gpui::test]
    fn derives_missing_colors_from_primary(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let package = package(directory.path());
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            let builtin = cx.global::<Catalog>().packages[0].clone();
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let original = (cx.theme().tokens.slider_bar, cx.theme().tokens.slider_thumb);
                apply_colors(package.clone(), None, cx);
                let expected = if mode.is_dark() { "#ddbc70" } else { "#b58128" };
                assert_eq!(cx.theme().primary, Hsla::parse_hex(expected).unwrap());
                assert_eq!(cx.theme().tokens.slider_bar, cx.theme().tokens.primary);
                assert_eq!(cx.theme().tokens.slider_thumb, original.1);
                assert_eq!(cx.theme().tokens.slider_thumb.color.a, 1.);
                assert!(cx.theme().tokens.slider_thumb.color.l > 0.9);
                apply_colors(builtin.clone(), None, cx);
                assert_eq!(cx.theme().tokens.slider_bar, original.0);
                assert_eq!(cx.theme().tokens.slider_thumb, original.1);
            }
        });
    }

    #[gpui::test]
    fn preserves_explicit_package_colors(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let mut package = package(directory.path());
        for config in [&mut package.light, &mut package.dark] {
            let colors = &mut Rc::make_mut(config).colors;
            colors.slider_bar = Some("#37b298".into());
            colors.slider_thumb = Some("#c6f8ee".into());
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            apply_colors(package, None, cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                assert_eq!(
                    cx.theme().tokens.slider_bar.color,
                    Hsla::parse_hex("#37b298").unwrap()
                );
                assert_eq!(
                    cx.theme().tokens.slider_thumb.color,
                    Hsla::parse_hex("#c6f8ee").unwrap()
                );
            }
        });
    }
}
