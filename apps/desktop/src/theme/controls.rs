use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{ThemeMode, button::ButtonVariants as _};
use gpui_kit::*;

#[gpui::test]
fn disabled_fill(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            for disabled in [false, true] {
                let mut button = send_button(Button::new("send").primary(), disabled, false, cx);
                let fill = (disabled && !mode.is_dark())
                    .then_some(Fill::from(cx.theme().secondary_active));
                assert_eq!(button.style().background, fill);
                assert_eq!(
                    button.style().border_color,
                    (disabled && !mode.is_dark()).then_some(cx.theme().secondary_active)
                );
            }
        }
    });
}

#[gpui::test]
fn package_switches(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        let builtin = cx.global::<Catalog>().packages[0].clone();
        let mut package = builtin.clone();
        package.id = "custom".into();
        for (config, selected, hover) in [
            (&mut package.light, "#f4e5f218", "#e4f4f518"),
            (&mut package.dark, "#35223530", "#123b4530"),
        ] {
            let colors = &mut Rc::make_mut(config).colors;
            colors.secondary_active = Some(selected.into());
            colors.list_active = Some(selected.into());
            colors.table_active = Some(selected.into());
            colors.accent = Some(hover.into());
            colors.list_hover = Some(hover.into());
            colors.table_hover = Some(hover.into());
        }
        cx.update_global::<Catalog, _>(|catalog, _| catalog.packages.push(package.clone()));
        let before_dark = serde_json::to_value(&builtin.dark.colors).unwrap();
        for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
            Theme::change(mode, None, cx);
            let defaults = (cx.theme().secondary_active, cx.theme().accent);
            apply(1, None, cx);
            let theme = cx.theme();
            let expected = gpui_kit::component::theme::try_parse_color(if mode.is_dark() {
                "#35223530"
            } else {
                "#f4e5f218"
            })
            .unwrap();
            assert_eq!(theme.secondary_active, expected);
            assert_eq!(theme.list_active, expected);
            assert_eq!(theme.table_active, expected);
            assert_ne!(theme.accent, theme.secondary_active);
            if !mode.is_dark() {
                assert_eq!(sidebar_item(true, false, cx), theme.secondary_active);
                assert_eq!(sidebar_item(false, true, cx), theme.accent);
            }
            apply(0, None, cx);
            assert_eq!((cx.theme().secondary_active, cx.theme().accent), defaults);
            assert_eq!(
                serde_json::to_value(&cx.global::<Catalog>().packages[0].dark.colors).unwrap(),
                before_dark
            );
        }
    });
}
