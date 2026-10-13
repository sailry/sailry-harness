use super::*;
use gpui_kit::{TestAppContext, component::ThemeMode, gpui};

#[gpui::test]
fn separates_diagram_ink_from_outlines(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let doc = crate::markdown::parse::parse("```mermaid\nflowchart LR\nA-->B\n```\n\n$$x^2$$");
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            gpui_kit::component::Theme::change(mode, None, cx);
            let theme = Theme::of(cx);
            let items = requests(&doc, &theme, mode == ThemeMode::Dark, 14.);
            let diagram = items
                .iter()
                .find(|item| item.kind == Kind::Mermaid)
                .unwrap();
            assert_eq!(
                diagram.foreground,
                color(theme.surface.blend(theme.text_muted))
            );
            assert_eq!(diagram.accent, color(theme.surface.blend(theme.border)));
            let formula = items
                .iter()
                .find(|item| item.kind == Kind::DisplayMath)
                .unwrap();
            assert_eq!(formula.foreground, color(theme.text));
            assert_eq!(formula.accent, color(theme.accent));
            let mut changed = theme.clone();
            changed.border = gpui_kit::rgb(0x123456).into();
            changed.text_muted = gpui_kit::rgb(0x654321).into();
            let next = requests(&doc, &changed, mode == ThemeMode::Dark, 14.);
            let next_diagram = next.iter().find(|item| item.kind == Kind::Mermaid).unwrap();
            assert_eq!(next_diagram.accent, "#123456");
            assert_eq!(next_diagram.foreground, "#654321");
            assert_ne!(key(diagram), key(next_diagram));
            assert_eq!(
                formula,
                next.iter()
                    .find(|item| item.kind == Kind::DisplayMath)
                    .unwrap()
            );
        }
    });
}

#[gpui::test]
fn preserves_translucent_diagram_colors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let doc = crate::markdown::parse::parse("```mermaid\nflowchart LR\nA-->B\n```");
        for (dark, surface, ink, outline, label) in [
            (false, 0xffffff, 0x000000, "#efefef", "#666666"),
            (true, 0x181818, 0xffffff, "#262626", "#a3a3a3"),
        ] {
            let mut theme = Theme::of(cx);
            theme.surface = gpui_kit::rgb(surface).into();
            theme.border = gpui_kit::Hsla::from(gpui_kit::rgb(ink)).alpha(16. / 255.);
            theme.text_muted = gpui_kit::Hsla::from(gpui_kit::rgb(ink)).alpha(0.6);
            let items = requests(&doc, &theme, dark, 14.);
            let diagram = &items[0];
            assert_eq!(diagram.accent, outline);
            assert_eq!(diagram.foreground, label);
            theme.border = theme.border.alpha(0.);
            let transparent = requests(&doc, &theme, dark, 14.);
            assert_eq!(transparent[0].accent, color(theme.surface));
            assert_ne!(key(diagram), key(&transparent[0]));
        }
    });
}
