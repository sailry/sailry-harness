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
            assert_eq!(diagram.foreground, color(theme.text_muted));
            assert_eq!(diagram.accent, color(theme.border));
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
