//! Bezel's content roles resolved from the active Sailry theme at paint time.
use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, FontWeight, Hsla, SharedString, Styled, px};

#[derive(Clone)]
pub struct Theme {
    pub text: Hsla,
    pub link: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub selection: Hsla,
    pub caret: Hsla,
    pub accent: Hsla,
    pub solid: Hsla,
    pub on_solid: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub busy: Hsla,
    pub code_text: Hsla,
    pub code_wash: Hsla,
    pub element_hover: Hsla,
    pub element_active: Hsla,
    pub surface: Hsla,
    pub surface_card: Hsla,
    pub font_body: SharedString,
    pub font_mono: SharedString,
    pub body_size: f32,
    pub code_size: f32,
    pub radius: f32,
}

impl Theme {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            text: theme.foreground,
            link: theme.link,
            text_muted: theme.muted_foreground,
            text_faint: theme.muted_foreground.opacity(0.7),
            border: theme.border,
            border_strong: theme.border,
            selection: theme.selection,
            caret: theme.foreground,
            accent: theme.accent,
            solid: theme.primary,
            on_solid: theme.primary_foreground,
            success: theme.success,
            warning: theme.warning,
            danger: theme.danger,
            busy: theme.blue,
            code_text: theme.foreground,
            code_wash: theme.muted,
            element_hover: theme.secondary_hover,
            element_active: theme.secondary_active,
            surface: theme.background,
            surface_card: theme.background,
            font_body: theme.font_family.clone(),
            font_mono: theme.mono_font_family.clone(),
            // Match Kit's small text scale: 14px at the default 16px theme size.
            body_size: f32::from(theme.font_size) * 0.875,
            code_size: theme.mono_font_size.into(),
            radius: theme.radius.into(),
        }
    }

    pub fn ink(&self, opacity: f32) -> Hsla {
        self.text.opacity(opacity)
    }
    pub fn hairline(&self, opacity: f32) -> Hsla {
        self.text.opacity(opacity)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextStyle {
    Body,
    Title,
    Title2,
    Title3,
    Headline,
    Callout,
    Subheadline,
    Caption,
}

impl TextStyle {
    fn size(self) -> f32 {
        match self {
            Self::Body | Self::Headline => 13.,
            Self::Title => 22.,
            Self::Title2 => 17.,
            Self::Title3 => 15.,
            Self::Callout => 12.,
            Self::Subheadline => 11.,
            Self::Caption => 10.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    size: f32,
    leading: f32,
    pub weight: FontWeight,
}

impl Metrics {
    pub fn new(role: TextStyle, leading: f32, weight: FontWeight) -> Self {
        Self {
            size: role.size(),
            leading,
            weight,
        }
    }
    pub fn size(self) -> f32 {
        self.size
    }
    pub fn line_height(self) -> f32 {
        self.size * self.leading
    }
    pub fn scaled(self, factor: f32) -> Self {
        Self {
            size: self.size * factor,
            ..self
        }
    }
}

pub trait Typeset: Styled + Sized {
    fn text_style(self, role: TextStyle, theme: &Theme) -> Self {
        self.text_size(px(role.size() * theme.body_size / 13.))
    }
}
impl<T: Styled> Typeset for T {}
