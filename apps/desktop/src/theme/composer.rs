use super::{Catalog, decoration};
use gpui_kit::{
    component::{button::Button, *},
    *,
};

pub(crate) fn surface(cx: &App) -> Div {
    let width = cx
        .try_global::<Catalog>()
        .and_then(|catalog| catalog.packages[catalog.selected].manifest.as_ref())
        .map(|manifest| {
            if cx.theme().is_dark() {
                &manifest.dark
            } else {
                &manifest.light
            }
        })
        .map_or(0, |variant| variant.composer.border_width);
    v_flex()
        .relative()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius_2xl())
        .bg(cx.theme().muted)
        .border(px(f32::from(width)))
        .border_color(cx.theme().input)
        .children(decoration::corner("composer.top_left", cx))
        .children(decoration::corner("composer.bottom_right", cx))
}

pub(super) fn content(button: Button, disabled: bool, active: bool, cx: &App) -> Button {
    let key = if active { "stop" } else { "send" };
    let slot = if active { "button.stop" } else { "button.send" };
    let style = super::variant(cx).and_then(|variant| {
        if active {
            variant.composer.stop.as_ref()
        } else {
            variant.composer.send.as_ref()
        }
    });
    let button = if let Some(style) = style {
        button.custom(appearance(style, cx))
    } else {
        button
    };
    let fallback = Icon::new(if active {
        IconName::WindowMaximize
    } else {
        IconName::ArrowUp
    });
    let fallback = if let Some(style) = style {
        fallback.text_color(if disabled {
            cx.theme().muted_foreground.opacity(0.5)
        } else {
            color(&style.foreground)
        })
    } else {
        fallback
    };
    let custom_icon = decoration::selected(&format!("icon.{key}"), cx).is_some();
    let texture = decoration::background(slot, cx).map(|texture| {
        // Blend textures over Kit's fill so pointer and disabled states stay visible.
        div().absolute().inset_0().opacity(0.6).child(texture)
    });
    if !custom_icon && texture.is_none() {
        return button.icon(fallback);
    }
    // Kit's icon slot accepts vector Icons, not raster images. Custom children
    // supply decoration only; Kit still owns focus, keyboard and disabled states.
    button.relative().size_8().p_0().overflow_hidden().child(
        div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(if disabled { 0.35 } else { 1. })
            .children(texture)
            .child(
                div()
                    .relative()
                    .child(decoration::sized_icon(key, rems(1.), fallback, cx)),
            ),
    )
}

fn color(value: &str) -> Hsla {
    theme::try_parse_color(value).expect("validated composer action color")
}

fn appearance(style: &super::package::Action, cx: &App) -> button::ButtonCustomVariant {
    button::ButtonCustomVariant::new(cx)
        .color(color(&style.background))
        .foreground(color(&style.foreground))
        .hover(color(&style.hover))
        .active(color(&style.active))
}
