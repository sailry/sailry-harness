//! Neutral surfaces share translucent theme tokens so the window material stays visible.
use gpui_kit::component::theme::ThemeConfigColors;

pub(super) fn configure(colors: &mut ThemeConfigColors, dark: bool) {
    let base = if dark { "#181818" } else { "#ffffff" };
    let foreground = if dark { "#ffffffd9" } else { "#000000e0" };
    let muted_foreground = if dark { "#ffffff99" } else { "#00000099" };
    let surface = if dark { "#ffffff0a" } else { "#00000006" };
    let inset = if dark { "#ffffff12" } else { "#00000006" };
    // Light hover retains the preceding selection depth; persistent selection
    // is lighter, rather than layering another heavy fill over the material.
    let hover = if dark { "#ffffff0f" } else { "#00000010" };
    let active = if dark { "#ffffff1f" } else { "#00000008" };
    let border = if dark { "#ffffff10" } else { "#00000010" };
    for color in [
        &mut colors.background,
        &mut colors.title_bar,
        &mut colors.status_bar,
    ] {
        *color = Some(base.into());
    }
    colors.sidebar = Some(if dark { "#1d1d1d" } else { "#fafafa" }.into());
    for color in [
        &mut colors.group_box,
        &mut colors.accordion,
        &mut colors.list,
        &mut colors.list_head,
        &mut colors.list_even,
        &mut colors.table_even,
        &mut colors.table_foot,
        &mut colors.tab_bar,
        &mut colors.secondary,
        &mut colors.button,
        &mut colors.button_secondary,
        &mut colors.description_list_label,
    ] {
        *color = Some(surface.into());
    }
    for color in [&mut colors.muted, &mut colors.tab_bar_segmented] {
        *color = Some(inset.into());
    }
    for color in [
        &mut colors.accent,
        &mut colors.list_hover,
        &mut colors.table_hover,
        &mut colors.secondary_hover,
        &mut colors.button_hover,
        &mut colors.button_secondary_hover,
    ] {
        *color = Some(hover.into());
    }
    colors.sidebar_accent = Some(if dark { "#ffffff0f" } else { active }.into());
    for color in [
        &mut colors.secondary_active,
        &mut colors.button_active,
        &mut colors.button_secondary_active,
        &mut colors.skeleton,
    ] {
        *color = Some(active.into());
    }
    for color in [
        &mut colors.border,
        &mut colors.input,
        &mut colors.status_bar_border,
        &mut colors.table_row_border,
        &mut colors.title_bar_border,
        &mut colors.window_border,
    ] {
        *color = Some(border.into());
    }
    // Shell boundaries need to remain visible over the native window material.
    colors.sidebar_border = Some(if dark { "#ffffff14" } else { "#00000014" }.into());
    for color in [
        &mut colors.foreground,
        &mut colors.accent_foreground,
        &mut colors.group_box_foreground,
        &mut colors.group_box_title_foreground,
        &mut colors.popover_foreground,
        &mut colors.secondary_foreground,
        &mut colors.button_foreground,
        &mut colors.button_secondary_foreground,
        &mut colors.sidebar_foreground,
        &mut colors.sidebar_accent_foreground,
        &mut colors.table_head_foreground,
        &mut colors.table_foot_foreground,
        &mut colors.description_list_label_foreground,
        &mut colors.caret,
    ] {
        *color = Some(foreground.into());
    }
    for color in [
        &mut colors.button_primary,
        &mut colors.sidebar_primary,
        &mut colors.progress_bar,
    ] {
        *color = Some(foreground.into());
    }
    // Kit's semantic primary is the configurable emphasis color. Its accent
    // token is a hover surface; filled controls retain their neutral overrides.
    colors.link = Some(if dark { "blue-300" } else { "blue-600" }.into());
    colors.link_active = Some(if dark { "blue-200" } else { "blue-700" }.into());
    colors.primary = Some(if dark { "blue-400" } else { "blue-600" }.into());
    colors.primary_hover = Some(if dark { "blue-300" } else { "blue-500" }.into());
    colors.primary_active = Some(if dark { "blue-500" } else { "blue-700" }.into());
    colors.button_primary_hover = Some(if dark { "#fffffff0" } else { "#000000f0" }.into());
    colors.button_primary_active = Some(if dark { "#ffffffff" } else { "#000000ff" }.into());
    for color in [
        &mut colors.primary_foreground,
        &mut colors.button_primary_foreground,
        &mut colors.sidebar_primary_foreground,
    ] {
        *color = Some(base.into());
    }
    for color in [
        &mut colors.danger_foreground,
        &mut colors.info_foreground,
        &mut colors.success_foreground,
        &mut colors.warning_foreground,
    ] {
        *color = Some("#ffffff".into());
    }
    if !dark {
        colors.group_box = Some("#00000000".into());
        colors.button = Some("#00000000".into());
        colors.button_secondary = Some("#00000000".into());
    }
    colors.switch = Some(if dark { "#ffffff1f" } else { "#0000001f" }.into());
    // Kit resolves an omitted thumb color from the active theme background.
    colors.switch_thumb = None;
    colors.slider_thumb = Some(base.into());
    colors.slider_bar = Some(foreground.into());
    colors.ring = Some(muted_foreground.into());
    // Table and list selection share the same light treatment as selected
    // buttons. Dark remains unchanged, including its keyboard selection edge.
    colors.list_active = Some(if dark { "#ffffff33" } else { active }.into());
    colors.table_active = colors.list_active.clone();
    colors.selection = Some(if dark { "#ffffff4d" } else { "#00000018" }.into());
    colors.list_active_border = Some(if dark { muted_foreground } else { "#00000040" }.into());
    colors.table_active_border = colors.list_active_border.clone();
    colors.muted_foreground = Some(muted_foreground.into());
    colors.tab_foreground = Some(muted_foreground.into());
    colors.tab_bar_segmented = Some(if dark { "#ffffff12" } else { "#00000008" }.into());
    colors.tab_active = Some(if dark { "#454545" } else { "#ffffff" }.into());
    colors.tab_active_foreground = Some(if dark { "#ffffff" } else { "#181818" }.into());
    colors.link_hover = Some(if dark { "#ffffff" } else { "#000000" }.into());
    // Overlays need stronger fills to separate their text from content underneath.
    colors.popover = Some(if dark { "#242424f5" } else { "#fffffff5" }.into());
    colors.tab = Some("#00000000".into());
    colors.scrollbar = Some("#00000000".into());
    colors.scrollbar_thumb = Some(border.into());
    colors.scrollbar_thumb_hover = Some(muted_foreground.into());
}
