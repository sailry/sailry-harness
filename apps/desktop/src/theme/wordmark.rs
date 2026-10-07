use super::variant;
use gpui_kit::{component::*, *};

pub(crate) fn colors(cx: &App) -> (Hsla, Hsla) {
    let style = variant(cx).map(|variant| &variant.wordmark);
    let parse = |value: Option<&String>, fallback| {
        value.map_or(fallback, |value| {
            theme::try_parse_color(value).expect("validated wordmark color")
        })
    };
    let ink = parse(
        style.and_then(|style| style.foreground.as_ref()),
        cx.theme().foreground,
    );
    (ink, parse(style.and_then(|style| style.glow.as_ref()), ink))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui_kit::test]
    fn follows_package_and_mode(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let source = crate::theme::fixture::styled(directory.path(), "wordmark");
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
                let (ink, glow) = colors(cx);
                assert_ne!(ink, cx.theme().foreground);
                assert_ne!(ink, glow);
                crate::theme::apply(0, None, cx);
                assert_eq!(colors(cx), (cx.theme().foreground, cx.theme().foreground));
            }
        });
    }
}
