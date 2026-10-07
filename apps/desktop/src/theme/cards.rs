use super::{Catalog, decoration};
use gpui_kit::{
    component::{button::ButtonCustomVariant, *},
    *,
};

pub(crate) struct Appearance {
    pub background: Hsla,
    pub foreground: Hsla,
    pub detail: Hsla,
    pub icon: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Hsla,
    pub border_width: Pixels,
    pub radius: Pixels,
}

pub(crate) fn appearance(key: &str, color: Hsla, cx: &App) -> Appearance {
    let style = cx
        .try_global::<Catalog>()
        .and_then(|catalog| catalog.packages[catalog.selected].manifest.as_ref())
        .and_then(|manifest| {
            let variant = if cx.theme().is_dark() {
                &manifest.dark
            } else {
                &manifest.light
            };
            variant.cards.get(key)
        });
    let parse = |value: Option<&String>, fallback| {
        value.map_or(fallback, |value| {
            theme::try_parse_color(value).expect("validated card color")
        })
    };
    Appearance {
        background: parse(style.and_then(|style| style.background.as_ref()), color),
        foreground: parse(
            style.and_then(|style| style.foreground.as_ref()),
            cx.theme().foreground,
        ),
        detail: parse(
            style.and_then(|style| style.detail.as_ref()),
            cx.theme().muted_foreground,
        ),
        icon: parse(style.and_then(|style| style.icon.as_ref()), color),
        hover: parse(
            style.and_then(|style| style.hover.as_ref()),
            color.opacity(0.2),
        ),
        active: parse(
            style.and_then(|style| style.active.as_ref()),
            color.opacity(0.26),
        ),
        border: parse(
            style.and_then(|style| style.border.as_ref()),
            cx.theme().border,
        ),
        border_width: px(f32::from(style.map_or(0, |style| style.border_width))),
        radius: style
            .and_then(|style| style.radius)
            .map_or(cx.theme().radius_2xl(), |radius| px(f32::from(radius))),
    }
}

impl Appearance {
    pub fn variant(&self, cx: &App) -> ButtonCustomVariant {
        ButtonCustomVariant::new(cx)
            .color(self.background)
            .foreground(self.foreground)
            .hover(self.hover)
            .active(self.active)
    }

    pub fn image(&self, key: &str, icon: IconName, cx: &App) -> AnyElement {
        decoration::sized_icon(key, rems(1.5), Icon::new(icon).text_color(self.icon), cx)
    }
}
