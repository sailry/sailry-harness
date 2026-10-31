use super::{Images, Kind};
use crate::markdown::{doc::Text, render::OnMathLink, style::Theme};
use gpui_kit::base::{TextView, text::TextViewStyle};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    SelectionFormat, markdown_ast,
};
use gpui_kit::{AnyElement, App, HighlightStyle, IntoElement, Window, div, img, prelude::*, px};
use std::sync::Arc;

struct Math {
    images: Arc<Images>,
}

impl MarkdownPlugin for Math {
    fn name(&self) -> &str {
        "formula"
    }
    fn parse(
        &self,
        node: &markdown_ast::Node,
        _: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::InlineMath(math) = node else {
            return None;
        };
        Some(
            MarkdownNode::new("formula", math.value.clone())
                .text(format!("${}$", math.value))
                .markdown(format!("${}$", math.value)),
        )
    }
    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> Option<InlineElement> {
        let source = node.data::<String>()?;
        let prepared = self.images.get(&(Kind::InlineMath, source.clone()))?;
        let scale = f32::from(context.font_size()) / prepared.font_size;
        Some(
            InlineElement::new(
                img(prepared.image.clone())
                    .w(px(prepared.width * scale))
                    .h(px(prepared.height * scale)),
            )
            .with_baseline(px(prepared.baseline.unwrap_or(prepared.height) * scale)),
        )
    }
}

/// Kit's default style carries a light-palette inline code background that
/// wins over the code background, so inline code takes the theme's code colors
/// explicitly, as the ordinary paragraph path does.
fn style(theme: &Theme, dark: bool) -> TextViewStyle {
    TextViewStyle::default()
        .with_foreground(theme.text)
        .with_muted_foreground(theme.text_muted)
        .with_link(theme.link)
        .with_selection(theme.selection)
        .with_code_background(theme.code_wash)
        .with_inline_code(HighlightStyle {
            color: Some(theme.code_text),
            background_color: Some(theme.code_wash),
            ..Default::default()
        })
        .with_border(theme.border)
        .with_dark(dark)
        .with_paragraph_gap(gpui_kit::rems(0.0))
}

pub(in crate::markdown) fn render(
    text: &Text,
    block: usize,
    images: &Images,
    link: Option<OnMathLink>,
    (size, line_height): (f32, f32),
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let view = TextView::markdown(
        format!("markdown-formulas-{block}"),
        crate::editor::serialize::text(text),
    )
    .selection_format(SelectionFormat::Source)
    .style(style(theme, cx.theme().is_dark()))
    .plugin(Math {
        images: Arc::new(images.clone()),
    })
    .when_some(link, |view, link| {
        view.on_link_click(move |url, _, window, cx| link(url, window, cx))
    });
    div()
        .min_w_0()
        .w_full()
        .text_size(px(size))
        .line_height(px(line_height))
        .child(view)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::{Theme as Kit, ThemeColor};

    #[test]
    fn code_uses_theme_colors() {
        for (colors, dark) in [(ThemeColor::light(), false), (ThemeColor::dark(), true)] {
            let theme = Theme::from_kit(&Kit::from(&*colors));
            let code = style(&theme, dark).inline_code();
            assert_eq!(code.color, Some(theme.code_text));
            assert_eq!(code.background_color, Some(theme.code_wash));
        }
        let light = Theme::from_kit(&Kit::from(&*ThemeColor::light()));
        let dark = Theme::from_kit(&Kit::from(&*ThemeColor::dark()));
        assert_ne!(light.code_wash, dark.code_wash);
    }
}
