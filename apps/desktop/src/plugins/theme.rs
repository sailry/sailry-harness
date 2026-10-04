//! Kit b79f4ce's script theme snapshot drops alpha from every color. Expose the
//! same semantic theme through Sailry's host module, preserving RGBA instead of
//! asking plugins to maintain another palette or flatten translucent surfaces.
use gpui_kit::{App, Hsla, Rgba, component::ActiveTheme};
use gpui_shell::{HostObject, HostValue};

pub(super) fn snapshot(cx: &App) -> HostValue {
    let theme = cx.theme();
    let colors = theme.semantic_tokens().colors;
    let mut values = HostObject::new();
    for (name, color) in [
        ("background", colors.background),
        ("foreground", colors.foreground),
        ("surface", colors.surface),
        ("surface_foreground", colors.surface_foreground),
        ("primary", colors.primary),
        ("primary_foreground", colors.primary_foreground),
        ("secondary", colors.secondary),
        ("secondary_foreground", colors.secondary_foreground),
        ("muted", colors.muted),
        ("muted_foreground", colors.muted_foreground),
        ("accent", colors.accent),
        ("accent_foreground", colors.accent_foreground),
        ("destructive", colors.destructive),
        ("destructive_foreground", colors.destructive_foreground),
        ("border", colors.border),
        ("input", colors.input),
        ("ring", colors.ring),
        ("selection", colors.selection),
        ("group_box", theme.group_box),
        ("group_box_foreground", theme.group_box_foreground),
        ("success", theme.success),
        ("warning", theme.warning),
        ("info", theme.info),
        ("chart_1", theme.chart_1),
        ("chart_2", theme.chart_2),
        ("chart_3", theme.chart_3),
        ("chart_4", theme.chart_4),
        ("chart_5", theme.chart_5),
    ] {
        values = values.field(name, rgba(color));
    }
    HostObject::new()
        .field("is_dark", theme.is_dark())
        .field("colors", values)
        .into()
}

fn rgba(color: Hsla) -> String {
    let color = Rgba::from(color);
    let byte = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b),
        byte(color.a)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        TestAppContext,
        component::{Theme, ThemeMode},
    };

    #[gpui_kit::test]
    fn preserves_semantic_alpha(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let value = snapshot(cx);
                let object = value.as_object().unwrap();
                let colors = object
                    .iter()
                    .find(|(key, _)| key == "colors")
                    .unwrap()
                    .1
                    .as_object()
                    .unwrap();
                for (name, expected) in [
                    ("border", cx.theme().border),
                    ("muted", cx.theme().muted),
                    ("foreground", cx.theme().foreground),
                    ("group_box_foreground", cx.theme().group_box_foreground),
                ] {
                    let actual = colors
                        .iter()
                        .find(|(key, _)| key == name)
                        .unwrap()
                        .1
                        .as_str()
                        .unwrap();
                    assert_eq!(actual, rgba(expected));
                    assert_eq!(actual.len(), 9);
                }
            }
        });
    }
}
