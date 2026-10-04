use std::rc::Rc;

use gpui_kit::Styled as _;
use gpui_kit::component::{
    ActiveTheme, Colorize as _, Disableable as _, Theme,
    button::{Button, ButtonCustomVariant},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Global, Hsla, Window, WindowAppearance};

mod catalog;
#[cfg(test)]
mod controls;
mod decoration;
mod dialog;
mod neutral;
#[cfg(test)]
mod tabs;
pub(crate) use dialog::DialogStyle;
#[cfg(test)]
pub(crate) mod fixture;
pub(crate) mod package;
pub(crate) use catalog::{Catalog, Mode, Package, Selection, apply, remove};
pub(crate) use decoration::preview;
pub(crate) use decoration::{background, banner, brand, icon};

#[derive(Default)]
struct SystemAppearance(bool);
impl Global for SystemAppearance {}

pub fn follows_system(cx: &App) -> bool {
    cx.try_global::<SystemAppearance>()
        .is_some_and(|mode| mode.0)
}

pub fn select(mode: Option<gpui_kit::component::ThemeMode>, window: &mut Window, cx: &mut App) {
    cx.set_global(SystemAppearance(mode.is_none()));
    crate::preferences::update(cx, |data| {
        data.appearance.mode = match mode {
            None => Mode::System,
            Some(gpui_kit::component::ThemeMode::Light) => Mode::Light,
            Some(gpui_kit::component::ThemeMode::Dark) => Mode::Dark,
        };
    });
    if let Some(mode) = mode {
        Theme::change(mode, Some(window), cx);
    } else {
        cx.set_window_appearance(None);
        Theme::change(cx.window_appearance(), Some(window), cx);
    }
}

/// Panel tint only; child text and interactive controls retain full opacity.
pub fn panel_background(cx: &App) -> Hsla {
    let surfaces = crate::preferences::data(cx).surfaces.unwrap_or_default();
    cx.theme().background.opacity(surfaces.main / 100.)
}

/// One continuous outer material for the title bar and feature rail.
pub fn sidebar_background(cx: &App) -> Hsla {
    let surfaces = crate::preferences::data(cx).surfaces.unwrap_or_default();
    cx.theme()
        .title_bar
        .opacity(surfaces.sidebar.unwrap_or(45.) / 100.)
}

pub fn navigation_background(cx: &App) -> Hsla {
    cx.theme().sidebar.opacity(panel_background(cx).a)
}

pub fn sidebar_item(selected: bool, hovered: bool, cx: &App) -> Hsla {
    let theme = cx.theme();
    if !theme.is_dark() {
        return match (selected, hovered) {
            (true, true) => theme.secondary_active.mix_oklab(theme.accent, 0.75),
            (true, false) => theme.secondary_active,
            (false, true) => theme.accent,
            (false, false) => theme.transparent,
        };
    }
    let opacity = match (selected, hovered) {
        (true, true) => 0.12,
        (true, false) => 0.06,
        (false, true) => 0.05,
        (false, false) => 0.0,
    };
    theme.sidebar_foreground.opacity(opacity)
}

pub fn subtle_button(cx: &App) -> ButtonCustomVariant {
    // Kit Ghost hover derives secondary instead of using its hover token. The
    // public custom variant keeps compact controls on the same semantic states.
    ButtonCustomVariant::new(cx)
        .foreground(cx.theme().foreground)
        .hover(cx.theme().accent)
        .active(cx.theme().secondary_active)
}

/// Kit b79f4ce fixes primary disabled fill at 15% of the enabled background
/// without a separate token. Its public Styled override lets the light composer
/// use the shared selected surface while preserving Kit's disabled behavior.
pub fn send_button(button: Button, disabled: bool, cx: &App) -> Button {
    button
        .disabled(disabled)
        .when(disabled && !cx.theme().is_dark(), |button| {
            button
                .bg(cx.theme().secondary_active)
                .border_color(cx.theme().secondary_active)
        })
}

pub fn init(cx: &mut App) {
    crate::ui::material::init(cx);
    let selection = crate::preferences::data(cx).appearance;
    cx.set_global(SystemAppearance(selection.mode == Mode::System));
    let mode = {
        let theme = Theme::global_mut(cx);
        theme.list.active_highlight = false;
        theme.focus_ring = false;
        theme.notification.content_width = true;
        neutral::configure(&mut Rc::make_mut(&mut theme.light_theme).colors, false);
        neutral::configure(&mut Rc::make_mut(&mut theme.dark_theme).colors, true);
        // Kit keeps omitted sizing fields from the preceding config. Capture the
        // application defaults so switching packages or variants cannot inherit them.
        for config in [&mut theme.light_theme, &mut theme.dark_theme] {
            let config = Rc::make_mut(config);
            let colors = &mut config.colors;
            // Kit DataTable uses these semantic tokens without a per-table fill override.
            colors.table = Some("#00000000".into());
            colors.table_head = Some("#00000000".into());
            colors.chart_1 = Some("amber-500".into());
            colors.chart_2 = Some("blue-500".into());
            colors.chart_3 = Some("emerald-500".into());
            colors.chart_4 = Some("violet-500".into());
            colors.chart_5 = Some("rose-500".into());
            config.font_size.get_or_insert(f32::from(theme.font_size));
            config.font_family.get_or_insert(theme.font_family.clone());
            config
                .mono_font_size
                .get_or_insert(f32::from(theme.mono_font_size));
            config
                .mono_font_family
                .get_or_insert(theme.mono_font_family.clone());
            config
                .radius
                .get_or_insert(f32::from(theme.radius) as usize);
            config
                .radius_lg
                .get_or_insert(f32::from(theme.radius_lg) as usize);
            config.shadow.get_or_insert(theme.shadow);
        }
        match selection.mode {
            Mode::Light => gpui_kit::component::ThemeMode::Light,
            Mode::Dark => gpui_kit::component::ThemeMode::Dark,
            Mode::System => cx.window_appearance().into(),
        }
    };
    Theme::change(mode, None, cx);
    let catalog = Catalog::new(cx);
    let package = catalog.packages[catalog.selected].clone();
    cx.set_global(catalog);
    catalog::apply_colors(package, None, cx);
    sync_window(cx);
    cx.observe_global::<Theme>(sync_window).detach();
}

pub fn terminal(cx: &App) -> sailry_protocol::terminal::Appearance {
    use sailry_protocol::terminal::{Appearance, ColorScheme, Rgb};
    let theme = cx.theme();
    let rgb = |color: Hsla| {
        // Terminal ANSI colors have no alpha channel; resolve theme overlays first.
        let color = theme.background.blend(color).to_rgb();
        Rgb {
            red: (color.r * 255.).round() as u8,
            green: (color.g * 255.).round() as u8,
            blue: (color.b * 255.).round() as u8,
        }
    };
    Appearance {
        foreground: rgb(theme.foreground),
        background: rgb(theme.background),
        color_scheme: if theme.is_dark() {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        },
        palette: [
            theme.background,
            theme.red,
            theme.green,
            theme.yellow,
            theme.blue,
            theme.magenta,
            theme.cyan,
            theme.foreground,
            theme.muted_foreground,
            theme.red_light,
            theme.green_light,
            theme.yellow_light,
            theme.blue_light,
            theme.magenta_light,
            theme.cyan_light,
            theme.foreground,
        ]
        .map(rgb),
    }
}

/// Editor surfaces are translucent overlays of the active semantic foreground.
/// Kit exposes them through its shared highlight theme for every source editor.
fn sync_editor(cx: &mut App) {
    let theme = cx.theme();
    let gutter = theme
        .foreground
        .opacity(if theme.is_dark() { 0.035 } else { 0.025 });
    let active = theme
        .foreground
        .opacity(if theme.is_dark() { 0.065 } else { 0.04 });
    let style = &theme.highlight_theme.style;
    if style.editor_gutter_background == Some(gutter) && style.editor_active_line == Some(active) {
        return;
    }
    let theme = Theme::global_mut(cx);
    let style = &mut std::sync::Arc::make_mut(&mut theme.highlight_theme).style;
    style.editor_gutter_background = Some(gutter);
    style.editor_active_line = Some(active);
}

fn sync_window(cx: &mut App) {
    sync_editor(cx);
    let appearance = if cx.theme().is_dark() {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    };
    cx.set_window_appearance(if follows_system(cx) {
        None
    } else {
        Some(appearance)
    });
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, component::ThemeMode, gpui};

    #[gpui::test]
    fn light_interactions_are_subtle(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            Theme::change(ThemeMode::Light, None, cx);
            let theme = cx.theme();
            for color in [
                theme.accent,
                theme.list_hover,
                theme.table_hover,
                theme.button_hover,
            ] {
                assert!((color.a - 16. / 255.).abs() < 0.001);
            }
            for color in [
                theme.list_active,
                theme.table_active,
                theme.secondary_active,
            ] {
                assert!((color.a - 8. / 255.).abs() < 0.001);
                assert!(color.a < theme.accent.a);
            }
            assert!((theme.selection.a - 24. / 255.).abs() < 0.001);
            assert!((theme.sidebar_accent.a - 8. / 255.).abs() < 0.001);
            assert!((theme.border.a - 16. / 255.).abs() < 0.001);
            assert!((theme.muted.a - 6. / 255.).abs() < 0.001);
            assert_eq!(theme.semantic_tokens().colors.muted, theme.muted);
            assert!(theme.list_active_border.a > theme.list_active.a);
            assert!(theme.table_active_border.a > theme.table_active.a);
            assert!(theme.table_active_border.a < theme.ring.a);
            assert_eq!(theme.background.l, 1.);
            assert!((theme.sidebar.l - 250. / 255.).abs() < 0.001);
            assert!(theme.button_primary.a > 0.8);
            Theme::change(ThemeMode::Dark, None, cx);
            assert!((cx.theme().accent.a - 15. / 255.).abs() < 0.001);
            assert!((cx.theme().secondary_active.a - 31. / 255.).abs() < 0.001);
            assert!((cx.theme().list_active.a - 51. / 255.).abs() < 0.001);
            assert!((cx.theme().table_active.a - 51. / 255.).abs() < 0.001);
            assert!((cx.theme().muted.a - 18. / 255.).abs() < 0.001);
            assert!((cx.theme().border.a - 16. / 255.).abs() < 0.001);
            assert_eq!(cx.theme().table_active_border, cx.theme().ring);
        });
    }

    #[gpui::test]
    fn editor_overlays_follow_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                sync_editor(cx);
                let style = &cx.theme().highlight_theme.style;
                let gutter = style.editor_gutter_background.unwrap();
                let active = style.editor_active_line.unwrap();
                assert!(gutter.a > 0. && gutter.a < active.a && active.a < 0.1);
                assert_eq!(gutter.l, cx.theme().foreground.l);
                assert_eq!(active.l, cx.theme().foreground.l);
            }
        });
    }

    #[gpui::test]
    fn switch_thumb_contrast(cx: &mut TestAppContext) {
        let contrast = |first: Hsla, second: Hsla| {
            let luminance = |color: Hsla| {
                let color = color.to_rgb();
                let linear = |channel: f32| {
                    if channel <= 0.04045 {
                        channel / 12.92
                    } else {
                        ((channel + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
            };
            let first = luminance(first);
            let second = luminance(second);
            (first.max(second) + 0.05) / (first.min(second) + 0.05)
        };
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let theme = cx.theme();
                let surface = theme.background.blend(theme.group_box);
                let selected = surface.blend(theme.button_primary);
                assert!(contrast(selected, theme.button_primary_foreground) >= 4.5);
                assert_ne!(theme.button_primary, theme.primary);
                for color in [
                    theme.button_primary,
                    theme.tab_active,
                    theme.tab_active_foreground,
                ] {
                    let color = theme.background.blend(color).to_rgb();
                    assert_eq!(color.r, color.g);
                    assert_eq!(color.g, color.b);
                }
                assert_eq!(theme.tab_active.a, 1.);
                assert_eq!(theme.tab_active_foreground.a, 1.);
                assert_eq!(
                    theme.tab_active.l,
                    if mode == ThemeMode::Dark {
                        69. / 255.
                    } else {
                        1.
                    }
                );
                let bar = theme.background.blend(theme.tab_bar_segmented);
                assert!(theme.tab_active.l > bar.l);
                assert_eq!(
                    theme.tab_active_foreground.l,
                    if mode == ThemeMode::Dark {
                        1.
                    } else {
                        24. / 255.
                    }
                );
                assert!(contrast(theme.tab_active, theme.tab_active_foreground) >= 4.5);
                let thumb = theme.switch_thumb;
                assert_eq!(theme.switch_thumb.a, 1.);
                for (opacity, minimum) in [(1., 3.), (0.5, 1.5)] {
                    let rendered = surface.blend(theme.button_primary.opacity(opacity));
                    assert!(contrast(thumb, rendered) >= minimum);
                    let inactive = surface.blend(theme.switch.opacity(opacity)).to_rgb().r;
                    assert!((thumb.to_rgb().r - inactive).abs() > 0.1 * opacity);
                }
            }
        });
    }

    #[gpui::test]
    fn surface_contrast(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let theme = cx.theme();
                let background = theme.background.to_rgb().r;
                let composer = theme.background.blend(theme.muted).to_rgb().r;
                let card = theme.background.blend(theme.group_box).to_rgb().r;
                let navigation = navigation_background(cx);
                let main = panel_background(cx);
                assert!(main.a > 0. && main.a < 1.);
                assert_eq!(navigation.a, main.a);
                let navigation_contrast = (navigation.to_rgb().r - main.to_rgb().r).abs();
                assert!((0.01..0.03).contains(&navigation_contrast));
                assert!(sidebar_background(cx).a < 1.);
                assert!(theme.muted.a > 0. && theme.muted.a < 1.);
                if mode == ThemeMode::Dark {
                    assert!(navigation.to_rgb().r > main.to_rgb().r);
                    let popover = theme.background.blend(theme.popover).to_rgb().r;
                    assert!(popover - background > 0.03);
                    assert!((background - 24. / 255.).abs() < 0.001);
                    assert!(theme.group_box.a > 0. && theme.group_box.a < 1.);
                    assert!(card - background > 0.03);
                    assert!(composer - card > 0.02);
                } else {
                    assert!(navigation.to_rgb().r < main.to_rgb().r);
                    assert_eq!(theme.group_box.a, 0.);
                    assert_eq!(card, background);
                    assert!(background - composer > 0.02);
                }
            }
        });
    }

    #[gpui::test]
    fn terminal_preserves_neutral_levels(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let appearance = terminal(cx);
                let muted = appearance.palette[8];
                assert_ne!(muted, appearance.foreground);
                assert_ne!(muted, appearance.background);
                if mode == ThemeMode::Light {
                    assert!(muted.red > appearance.foreground.red);
                    assert!(muted.red < appearance.background.red);
                } else {
                    assert!(muted.red < appearance.foreground.red);
                    assert!(muted.red > appearance.background.red);
                }
            }
        });
    }

    #[gpui::test]
    fn shell_boundaries_remain_distinct(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let theme = cx.theme();
                for border in [
                    theme.title_bar_border,
                    theme.window_border,
                    theme.status_bar_border,
                    theme.input,
                    theme.table_row_border,
                ] {
                    assert_eq!(border, theme.border);
                }
                assert_eq!(theme.semantic_tokens().colors.border, theme.border);
                let base = cx.global::<gpui_kit::base::Theme>();
                assert_eq!(base.tokens.colors.border, theme.border);
                assert_eq!(base.tokens.colors.input, theme.border);
                assert_eq!(base.resizable.handle, Some(theme.border));
                assert!((theme.border.a - 16. / 255.).abs() < 0.001);
                assert!((theme.sidebar_border.a - 20. / 255.).abs() < 0.001);
                assert!(theme.border.a <= 0.1);
                assert!(theme.sidebar_border.a > theme.border.a);
                assert!(theme.sidebar_border.a <= 0.1);
                for surface in [theme.background, theme.sidebar] {
                    let edge = surface.blend(theme.sidebar_border).to_rgb().r;
                    let fill = surface.to_rgb().r;
                    let minimum = if mode.is_dark() { 0.06 } else { 0.04 };
                    assert!((edge - fill).abs() >= minimum);
                }
            }
        });
    }
}
