//! Semantic theme values for sandboxed authored HTML; authored CSS may override them.
use gpui_kit::{App, Hsla, Rgba, component::ActiveTheme};
use std::hash::{Hash, Hasher};

/// Custom properties named `--sailry-<name>` plus the matching `color-scheme`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    pub(crate) variables: Vec<(&'static str, String)>,
}

impl Palette {
    pub(crate) fn of(cx: &App) -> Self {
        let tokens = cx.theme();
        let mut variables: Vec<_> = [
            ("background", tokens.background),
            ("foreground", tokens.foreground),
            ("primary", tokens.primary),
            ("primary-foreground", tokens.primary_foreground),
            ("secondary", tokens.secondary),
            ("secondary-foreground", tokens.secondary_foreground),
            ("muted", tokens.muted),
            ("muted-foreground", tokens.muted_foreground),
            ("accent", tokens.accent),
            ("border", tokens.border),
            ("success", tokens.success),
            ("warning", tokens.warning),
            ("danger", tokens.danger),
            ("chart-1", tokens.chart_1),
            ("chart-2", tokens.chart_2),
            ("chart-3", tokens.chart_3),
            ("chart-4", tokens.chart_4),
            ("chart-5", tokens.chart_5),
        ]
        .into_iter()
        .map(|(name, color)| (name, rgba(color)))
        .collect();
        variables.extend([
            (
                "font-family",
                font_family(&tokens.font_family, "system-ui, sans-serif"),
            ),
            (
                "mono-font-family",
                font_family(&tokens.mono_font_family, "ui-monospace, monospace"),
            ),
            ("font-size", format!("{}px", f32::from(tokens.font_size))),
            ("radius", format!("{}px", f32::from(tokens.radius))),
        ]);
        Self {
            dark: tokens.is_dark(),
            variables,
        }
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn scheme(&self) -> &'static str {
        if self.dark { "dark" } else { "light" }
    }

    /// One `:root` rule whose values cannot close the surrounding style element.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn rule(&self) -> String {
        let mut rule = format!(":root{{color-scheme:{};", self.scheme());
        for (name, value) in &self.variables {
            rule.push_str("--sailry-");
            rule.push_str(name);
            rule.push(':');
            for character in value.chars() {
                match character {
                    '<' => rule.push_str("\\3c "),
                    '>' => rule.push_str("\\3e "),
                    character => rule.push(character),
                }
            }
            rule.push(';');
        }
        rule.push('}');
        rule
    }

    /// Identifies prepared output for this palette within one process.
    pub(crate) fn digest(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

pub(crate) fn rgba(color: Hsla) -> String {
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

pub(crate) fn font_family(family: &str, fallback: &str) -> String {
    // GPUI's virtual system family resolves natively, but is not a CSS font name.
    if family == ".SystemUIFont" || family.trim().is_empty() {
        return fallback.into();
    }
    let mut quoted = String::from("\"");
    for character in family.chars() {
        match character {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\0' => quoted.push('\u{fffd}'),
            character if character.is_control() => {
                quoted.push_str(&format!("\\{:x} ", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push_str("\", ");
    quoted.push_str(fallback);
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::{
        TestAppContext,
        component::{Theme, ThemeMode},
    };

    fn palette(dark: bool) -> Palette {
        Palette {
            dark,
            variables: vec![
                ("background", "#101010ff".into()),
                ("font-family", "\"A</style><script>\", system-ui".into()),
            ],
        }
    }

    #[test]
    fn rule_keeps_values_inside_style() {
        let rule = palette(true).rule();
        assert!(rule.starts_with(":root{color-scheme:dark;--sailry-background:#101010ff;"));
        assert!(!rule.contains('<') && !rule.contains('>'));
        assert!(rule.contains("\\3c /style\\3e "));
        assert!(rule.ends_with(";}"));
        assert!(palette(false).rule().contains("color-scheme:light;"));
    }

    #[test]
    fn digest_follows_values() {
        assert_eq!(palette(true).digest(), palette(true).digest());
        assert_ne!(palette(true).digest(), palette(false).digest());
        let mut changed = palette(true);
        changed.variables[0].1 = "#202020ff".into();
        assert_ne!(changed.digest(), palette(true).digest());
    }

    #[gpui_kit::test]
    fn follows_semantic_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            for (mode, dark) in [(ThemeMode::Light, false), (ThemeMode::Dark, true)] {
                Theme::change(mode, None, cx);
                let palette = Palette::of(cx);
                let value = |name: &str| {
                    palette
                        .variables
                        .iter()
                        .find(|(key, _)| *key == name)
                        .map(|(_, value)| value.clone())
                };
                assert_eq!(palette.dark, dark);
                assert_eq!(value("background"), Some(rgba(cx.theme().background)));
                assert_eq!(value("chart-1"), Some(rgba(cx.theme().chart_1)));
                assert!(
                    value("font-family")
                        .is_some_and(|family| family.ends_with("system-ui, sans-serif"))
                );
            }
        });
    }
}
