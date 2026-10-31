use super::*;
use gpui_kit::{TestAppContext, component::ThemeMode, gpui};

fn rendered(doc: &Doc, theme: &Theme, dark: bool) -> Vec<Request> {
    requests(doc, theme, dark, 14., &mut HashMap::new(), None)
        .into_iter()
        .map(|(request, _)| request)
        .collect()
}

fn contrast(left: &str, right: &str) -> f64 {
    let luminance = |color: &str| {
        [0.2126, 0.7152, 0.0722]
            .into_iter()
            .enumerate()
            .map(|(index, weight)| {
                let start = 1 + index * 2;
                let value = u8::from_str_radix(&color[start..start + 2], 16).unwrap() as f64 / 255.;
                weight
                    * if value <= 0.04045 {
                        value / 12.92
                    } else {
                        ((value + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum::<f64>()
    };
    let left = luminance(left);
    let right = luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

#[gpui::test]
fn separates_diagram_ink_from_outlines(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let doc = crate::markdown::parse::parse("```mermaid\nflowchart LR\nA-->B\n```\n\n$$x^2$$");
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            gpui_kit::component::Theme::change(mode, None, cx);
            let theme = Theme::of(cx);
            let items = rendered(&doc, &theme, mode == ThemeMode::Dark);
            let diagram = items
                .iter()
                .find(|item| item.kind == Kind::Mermaid)
                .unwrap();
            assert_eq!(diagram.foreground, color(theme.surface.blend(theme.text)));
            assert_eq!(diagram.accent, color(theme.surface.blend(theme.text_muted)));
            assert!(contrast(&diagram.foreground, &diagram.background) >= 4.5);
            assert!(contrast(&diagram.accent, &diagram.background) >= 3.);
            let formula = items
                .iter()
                .find(|item| item.kind == Kind::DisplayMath)
                .unwrap();
            assert_eq!(formula.foreground, color(theme.text));
            assert_eq!(formula.accent, color(theme.accent));
            let mut changed = theme.clone();
            changed.border = gpui_kit::rgb(0x123456).into();
            changed.text_muted = gpui_kit::rgb(0x654321).into();
            let next = rendered(&doc, &changed, mode == ThemeMode::Dark);
            let next_diagram = next.iter().find(|item| item.kind == Kind::Mermaid).unwrap();
            assert_eq!(next_diagram.accent, "#654321");
            assert_eq!(next_diagram.foreground, diagram.foreground);
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
            (false, 0xffffff, 0x000000, "#666666", "#000000"),
            (true, 0x181818, 0xffffff, "#a3a3a3", "#ffffff"),
        ] {
            let mut theme = Theme::of(cx);
            theme.surface = gpui_kit::rgb(surface).into();
            theme.text = gpui_kit::rgb(ink).into();
            theme.border = gpui_kit::Hsla::from(gpui_kit::rgb(ink)).alpha(16. / 255.);
            theme.text_muted = gpui_kit::Hsla::from(gpui_kit::rgb(ink)).alpha(0.6);
            let items = rendered(&doc, &theme, dark);
            let diagram = &items[0];
            assert_eq!(diagram.accent, outline);
            assert_eq!(diagram.foreground, label);
            theme.text_muted = theme.text_muted.alpha(0.);
            let transparent = rendered(&doc, &theme, dark);
            assert_eq!(transparent[0].accent, color(theme.surface));
            assert_ne!(key(diagram), key(&transparent[0]));
        }
    });
}

#[gpui::test]
fn renders_sanitized_html_for_its_source(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let source =
            "<div align=\"center\" onclick=\"run()\">\n<b>Hi</b><script>run()</script>\n</div>";
        let doc = crate::markdown::parse::parse(&format!("{source}\n\n<!-- note -->"));
        let mut sanitized = HashMap::new();
        let items = requests(&doc, &Theme::of(cx), false, 14., &mut sanitized, None);
        let [(request, original)] = items.as_slice() else {
            panic!("one HTML request: {items:?}");
        };
        assert_eq!(request.kind, Kind::Html);
        assert_eq!(original, source);
        assert!(request.source.contains("<b>Hi</b>"));
        assert!(!request.source.contains("onclick") && !request.source.contains("script"));
        assert_eq!(sanitized.len(), 2);
    });
}

#[gpui::test]
fn defers_blocks_showing_source(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let doc = crate::markdown::parse::parse("```mermaid\nflowchart LR\nA-->B\n```\n\n$$x^2$$");
        let theme = Theme::of(cx);
        let items = requests(&doc, &theme, false, 14., &mut HashMap::new(), Some((0, 0)));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0.kind, Kind::DisplayMath);
        let items = requests(&doc, &theme, false, 14., &mut HashMap::new(), None);
        assert_eq!(items.len(), 2);
    });
}
