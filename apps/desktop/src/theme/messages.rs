//! Surface refinements for Kit message slots; content and actions stay with the conversation.
use super::{decoration, variant};
use gpui_kit::{
    component::{bubble::BubbleContent, message::MessageContent, *},
    *,
};

fn style<T: Styled>(mut content: T, key: &str, cx: &App) -> T {
    let Some(style) = variant(cx).and_then(|variant| variant.messages.get(key)) else {
        return content;
    };
    let color = |value: &str| theme::try_parse_color(value).expect("validated message color");
    if let Some(value) = &style.background {
        content = content.bg(color(value));
    }
    if let Some(value) = &style.foreground {
        content = content.text_color(color(value));
    }
    if let Some(value) = &style.border {
        content = content.border_color(color(value));
    }
    if let Some(value) = style.border_width {
        content = content.border(px(f32::from(value)));
    }
    if let Some(value) = style.radius {
        content = content.rounded(px(f32::from(value)));
    }
    content
}

pub(crate) fn user(cx: &App) -> BubbleContent {
    style(
        BubbleContent::new()
            .px(px(14.))
            .py(px(9.))
            .rounded(cx.theme().radius),
        "user",
        cx,
    )
    .relative()
    .children(decoration::corner("message.user.top_left", cx))
    .children(decoration::corner("message.user.bottom_right", cx))
}

pub(crate) fn assistant(content: MessageContent, cx: &App) -> MessageContent {
    let radius = variant(cx)
        .and_then(|variant| variant.messages.get("assistant"))
        .and_then(|style| style.radius)
        .map_or(px(0.), |radius| px(f32::from(radius)));
    style(content, "assistant", cx).relative().child(
        div()
            .absolute()
            .inset_0()
            .rounded(radius)
            .overflow_hidden()
            .children(decoration::corner("message.assistant.top_left", cx))
            .children(decoration::corner("message.assistant.bottom_right", cx)),
    )
}

pub(crate) fn foreground(key: &str, cx: &App) -> Option<Hsla> {
    variant(cx)?
        .messages
        .get(key)?
        .foreground
        .as_ref()
        .map(|color| theme::try_parse_color(color).expect("validated message color"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn restores_default_surface(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let source = crate::theme::fixture::styled(directory.path(), "messages");
        let loaded = crate::theme::package::Loaded::read(&source, true).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            cx.global_mut::<crate::theme::Catalog>()
                .packages
                .push(loaded.into());
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                crate::theme::apply(1, None, cx);
                let mut content = user(cx);
                assert_eq!(content.style().border_widths.top, Some(px(1.).into()));
                assert_eq!(content.style().text.color, foreground("user", cx));
                assert!(content.style().background.is_some());
                crate::theme::apply(0, None, cx);
                let mut content = user(cx);
                assert!(content.style().background.is_none());
                assert!(content.style().text.color.is_none());
                assert!(foreground("assistant", cx).is_none());
            }
        });
    }
}
